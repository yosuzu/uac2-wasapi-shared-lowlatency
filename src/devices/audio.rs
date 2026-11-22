use std::{fmt::Write, ptr, slice};
use windows::Win32::{
    Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
    Foundation::{PROPERTYKEY, RPC_E_CHANGED_MODE},
    Media::Audio::*,
    System::{
        Com::{StructuredStorage::PropVariantToStringAlloc, *},
        Variant::{VT_BLOB, VT_UI4},
    },
    UI::Shell::PropertiesSystem::IPropertyStore,
};
use windows::core::{HSTRING, Interface, Result as WinResult};

fn enumerator() -> Result<IMMDeviceEnumerator, String> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
        .context("连接 Windows 音频枚举器")
}

fn prop_text(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    unsafe {
        let value = store.GetValue(key).ok()?;
        let s = task_string(PropVariantToStringAlloc(&value).ok()?).ok()?;
        (!s.is_empty()).then_some(s)
    }
}

fn inspect(
    ep: &IMMDevice,
    default_id: &str,
    read_snapshot: bool,
) -> Result<super::Endpoint, String> {
    let id = task_string(unsafe { ep.GetId() }.context("读取播放端点 ID")?)?;
    let render: IMMEndpoint = ep.cast().context("检查播放方向")?;
    if unsafe { render.GetDataFlow() }.context("读取方向")? != eRender
        || unsafe { ep.GetState() }.context("读取设备状态")? != DEVICE_STATE_ACTIVE
    {
        return Err("播放设备已离线。".into());
    }
    let mut data = super::Endpoint {
        name: id.clone(),
        id,
        form_factor: u32::MAX,
        ..Default::default()
    };
    data.is_default = data.id.eq_ignore_ascii_case(default_id);
    if let Ok(store) = unsafe { ep.OpenPropertyStore(STGM_READ) } {
        if let Some(n) = prop_text(&store, &PKEY_Device_FriendlyName) {
            data.name = n
        }
        data.association = prop_text(&store, &PKEY_AudioEndpoint_Association).unwrap_or_default();
        if let Ok(f) = unsafe { store.GetValue(&PKEY_AudioEndpoint_FormFactor) } {
            if f.vt() == VT_UI4 {
                data.form_factor = unsafe { f.Anonymous.Anonymous.Anonymous.ulVal }
            }
        }
        if let Some(f) = output_format(&store) {
            data.rate = f.rate;
            data.bits = f.valid_bits;
            data.channels = f.channels;
            data.container_bits = f.bits;
            data.encoding = f.encoding;
            data.raw_format = f.raw_hex;
        }
    }
    let binding = super::pnp::identify(&mut data);
    data.can_rebuild = binding.is_ok();
    let engine = periods(ep).map(|s| {
        data.mix_rate = s.mix.rate;
        data.mix_bits = s.mix.valid_bits;
        data.mix_channels = s.mix.channels;
        data.mix_encoding = s.mix.encoding;
        data.mix_raw = s.mix.raw_hex;
        data.min = s.min;
        data.default = s.default;
        data.fundamental = s.step;
        data.max = s.max;
        if let Some((n, f)) = s.running {
            data.current = n;
            data.current_rate = f.rate
        }
    });
    data.can_benchmark = engine.is_ok()
        && data.mix_rate != 0
        && data.mix_channels != 0
        && data.mix_bits != 0
        && !data.mix_raw.is_empty();
    data.reason = binding
        .and(engine)
        .and_then(|_| {
            if data.raw_format.is_empty() {
                Err("未读到 Windows 设备格式。".into())
            } else {
                Ok(())
            }
        })
        .err()
        .unwrap_or_default();
    data.supported = data.reason.is_empty();
    if read_snapshot {
        (data.snapshot, data.restart_pending) = crate::profile::snapshot_status(&data);
    }
    Ok(data)
}

pub(super) fn list(read_snapshot: bool) -> Result<Vec<super::Endpoint>, String> {
    let _com = Apartment::enter()?;
    let enumerator = enumerator()?;
    let default_id = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
        .ok()
        .and_then(|ep| unsafe { ep.GetId() }.ok())
        .and_then(|s| task_string(s).ok())
        .unwrap_or_default();
    let all = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }
        .context("枚举活动播放设备")?;
    let mut devices = Vec::new();
    for i in 0..unsafe { all.GetCount() }.context("读取播放设备数量")? {
        if let Ok(ep) = unsafe { all.Item(i) } {
            if let Ok(item) = inspect(&ep, &default_id, read_snapshot) {
                devices.push(item)
            }
        }
    }
    devices.sort_by_key(|d| (!d.is_default, d.name.clone(), d.id.clone()));
    Ok(devices)
}

pub(super) fn get(id: &str, read_snapshot: bool) -> Result<super::Endpoint, String> {
    let _com = Apartment::enter()?;
    let ep = unsafe { enumerator()?.GetDevice(&HSTRING::from(id)) }.context("查找所选设备")?;
    inspect(&ep, "", read_snapshot)
}

// Check only the two format blobs while the desktop is idle. Full PnP discovery
// and period queries are needed only when these change or an endpoint disappears.
pub(super) fn formats(id: &str) -> Result<(String, String), String> {
    let _com = Apartment::enter()?;
    unsafe {
        let ep = enumerator()?
            .GetDevice(&HSTRING::from(id))
            .context("查找播放设备")?;
        if ep.GetState().context("读取设备状态")? != DEVICE_STATE_ACTIVE {
            return Err("播放设备已离线。".into());
        }
        let output = ep
            .OpenPropertyStore(STGM_READ)
            .ok()
            .and_then(|s| output_format(&s));
        let mix = ep
            .Activate::<IAudioClient3>(CLSCTX_ALL, None)
            .ok()
            .and_then(|c| c.GetMixFormat().ok())
            .and_then(|p| TaskMem(p).format().ok());
        Ok((
            output.map(|f| f.raw_hex).unwrap_or_default(),
            mix.map(|f| f.raw_hex).unwrap_or_default(),
        ))
    }
}

pub(super) fn current(id: &str) -> Result<(u32, u32), String> {
    let _com = Apartment::enter()?;
    unsafe {
        let ep = enumerator()?
            .GetDevice(&HSTRING::from(id))
            .context("打开试流设备")?;
        let client: IAudioClient3 = ep.Activate(CLSCTX_ALL, None).context("打开音频查询")?;
        let mut buf = ptr::null_mut();
        let mut frames = 0;
        let result = client.GetCurrentSharedModeEnginePeriod(&mut buf, &mut frames);
        let mem = TaskMem(buf);
        result.context("读取当前引擎周期")?;
        Ok((mem.format()?.rate, frames))
    }
}
struct Format {
    rate: u32,
    channels: u16,
    valid_bits: u16,
    bits: u16,
    encoding: String,
    raw_hex: String,
}
trait WinContext<T> {
    fn context(self, action: &str) -> Result<T, String>;
}
impl<T> WinContext<T> for WinResult<T> {
    fn context(self, action: &str) -> Result<T, String> {
        self.map_err(|e| format!("{action}失败（HRESULT 0x{:08X}）。", e.code().0 as u32))
    }
}

struct Apartment(bool);
impl Apartment {
    fn enter() -> Result<Self, String> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        result.ok().context("初始化 Windows 音频查询")?;
        Ok(Self(true))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

struct TaskMem<T>(*mut T);
impl<T> Drop for TaskMem<T> {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.cast())) };
    }
}

fn task_string(value: windows::core::PWSTR) -> Result<String, String> {
    if value.is_null() {
        return Err("Windows 未返回音频设备 ID。".into());
    }
    let _allocation = TaskMem(value.0);
    unsafe { value.to_string() }.map_err(|_| "Windows 音频设备 ID 编码无效。".into())
}

fn parse_wave(data: &[u8]) -> Result<Format, String> {
    if data.len() < 16 {
        return Err("Windows 返回的音频格式首部被截断。".into());
    }
    let word = |pos: usize| u16::from_le_bytes([data[pos], data[pos + 1]]);
    let tag = word(0);
    let bits = word(14);
    let channels = word(2);
    let rate = u32::from_le_bytes(data[4..8].try_into().unwrap());
    let ext = if data.len() >= 18 {
        word(16) as usize
    } else {
        0
    };
    if channels == 0
        || rate == 0
        || bits == 0
        || data.len() == 17
        || (data.len() >= 18 && 18 + ext > data.len())
    {
        return Err("Windows 返回的音频格式无效。".into());
    }
    let (valid_bits, encoding) = if tag == 0xfffe {
        if ext < 22 {
            return Err("扩展音频格式缺少有效位深或子格式。".into());
        }
        let sub = unsafe {
            data[24..40]
                .as_ptr()
                .cast::<windows::core::GUID>()
                .read_unaligned()
        };
        let pcm = windows::core::GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
        let float = windows::core::GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
        let name = if sub == pcm {
            "PCM".into()
        } else if sub == float {
            "Float".into()
        } else {
            format!("{sub:?}")
        };
        (word(18), name)
    } else {
        let name = match tag {
            1 => "PCM".into(),
            3 => "Float".into(),
            _ => format!("0x{tag:04X}"),
        };
        (bits, name)
    };
    if valid_bits > bits {
        return Err("音频格式的有效位深超过了容器位深。".into());
    }
    let mut raw_hex = String::with_capacity(data.len() * 2);
    for b in data {
        write!(&mut raw_hex, "{b:02X}").unwrap();
    }
    Ok(Format {
        rate,
        channels,
        valid_bits,
        bits,
        encoding,
        raw_hex,
    })
}

impl TaskMem<WAVEFORMATEX> {
    fn format(&self) -> Result<Format, String> {
        if self.0.is_null() {
            return Err("Windows 未返回音频格式。".into());
        }
        // Both IAudioClient format methods return a CoTaskMem-allocated WAVEFORMATEX
        // with cbSize bytes of trailing format data.
        unsafe {
            let len = size_of::<WAVEFORMATEX>() + (*self.0).cbSize as usize;
            parse_wave(slice::from_raw_parts(self.0.cast(), len))
        }
    }
}

fn output_format(store: &IPropertyStore) -> Option<Format> {
    unsafe {
        let value = store.GetValue(&PKEY_AudioEngine_DeviceFormat).ok()?;
        if value.vt() != VT_BLOB {
            return None;
        }
        let blob = value.Anonymous.Anonymous.Anonymous.blob;
        if blob.pBlobData.is_null() || !(16..=65553).contains(&blob.cbSize) {
            return None;
        }
        parse_wave(slice::from_raw_parts(blob.pBlobData, blob.cbSize as usize)).ok()
    }
}

struct EnginePeriods {
    mix: Format,
    default: u32,
    step: u32,
    min: u32,
    max: u32,
    running: Option<(u32, Format)>,
}

fn periods(ep: &IMMDevice) -> Result<EnginePeriods, String> {
    unsafe {
        let client: IAudioClient3 = ep
            .Activate(CLSCTX_ALL, None)
            .context("读取端点的 IAudioClient3")?;
        let props = AudioClientProperties {
            cbSize: size_of::<AudioClientProperties>() as u32,
            eCategory: AudioCategory_Other,
            Options: AUDCLNT_STREAMOPTIONS_NONE,
            ..Default::default()
        };
        // SetClientProperties configures only this uninitialized client.
        client
            .SetClientProperties(&props)
            .context("准备普通 Shared 查询")?;
        let format_mem = TaskMem(client.GetMixFormat().context("读取共享混音格式")?);
        let mix = format_mem.format()?;
        let (mut default, mut step, mut min, mut max) = (0, 0, 0, 0);
        client
            .GetSharedModeEnginePeriod(format_mem.0, &mut default, &mut step, &mut min, &mut max)
            .context("读取共享引擎周期范围")?;
        if step == 0 || min == 0 || !(min..=max).contains(&default) {
            return Err("共享引擎返回的周期范围无效。".into());
        }
        let mut frames = 0;
        let mut current_ptr = ptr::null_mut();
        let status = client.GetCurrentSharedModeEnginePeriod(&mut current_ptr, &mut frames);
        let memory = TaskMem(current_ptr);
        let running = if status.is_ok() && frames != 0 {
            memory.format().ok().map(|format| (frames, format))
        } else {
            None
        };
        Ok(EnginePeriods {
            mix,
            default,
            step,
            min,
            max,
            running,
        })
    }
}
