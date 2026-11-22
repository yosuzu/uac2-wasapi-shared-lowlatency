use super::{Result, capability, store};
use crate::devices::Endpoint;
use std::{
    mem::ManuallyDrop, os::windows::process::CommandExt, path::PathBuf, process::Command, slice,
};
use windows::{
    Win32::{
        Devices::{DeviceAndDriverInstallation::*, Properties::*},
        Foundation::{
            CloseHandle, HANDLE, PROPERTYKEY, RPC_E_CHANGED_MODE, WAIT_ABANDONED, WAIT_OBJECT_0,
        },
        Media::Audio::{Endpoints::IAudioEndpointVolume, *},
        System::{
            Com::{StructuredStorage::*, *},
            SystemInformation::GetSystemDirectoryW,
            Threading::*,
            Variant::{VT_BLOB, VT_CLSID},
        },
        UI::Shell::PropertiesSystem::IPropertyStore,
    },
    core::{GUID, HSTRING, w},
};

fn err(e: windows::core::Error) -> String {
    format!("Windows 0x{:08X} ({})", e.code().0 as u32, e.message())
}

pub(super) struct Apartment(bool);
impl Apartment {
    pub fn enter() -> Result<Self> {
        let r = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if r == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        r.ok().map_err(err)?;
        Ok(Self(true))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() }
        }
    }
}

pub(super) struct Operation(HANDLE);
impl Operation {
    pub fn lock() -> Result<Self> {
        let h = unsafe {
            CreateMutexW(
                None,
                false,
                w!(r"Global\UAC2.SharedPeriod.ProfileOperation.v1"),
            )
        }
        .map_err(err)?;
        if !matches!(
            unsafe { WaitForSingleObject(h, 0) },
            WAIT_OBJECT_0 | WAIT_ABANDONED
        ) {
            unsafe {
                let _ = CloseHandle(h);
            }
            return Err("另一项设备操作正在进行。".into());
        }
        Ok(Self(h))
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

const PID2: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa2a3fff4_353f_407c_9d86_1f9dc7d5a606),
    pid: 2,
};
const CONTAINER: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x8c7ed206_3f8a_4827_b3ab_ae9e1faefc6c),
    pid: 2,
};

fn enumerator() -> Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(err)
}

fn id(ep: &IMMDevice) -> Result<String> {
    unsafe {
        let p = ep.GetId().map_err(err)?;
        if p.is_null() {
            return Err("未读到端点 ID。".into());
        }
        let s = p.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(p.0.cast()));
        s
    }
}

fn blob(store: &IPropertyStore, key: &PROPERTYKEY) -> Result<Vec<u8>> {
    unsafe {
        let v = store.GetValue(key).map_err(err)?;
        if v.vt() != VT_BLOB {
            return Err("端点属性未返回预期的二进制记录。".into());
        }
        let b = v.Anonymous.Anonymous.Anonymous.blob;
        if b.pBlobData.is_null() || !(1..=1024 * 1024).contains(&b.cbSize) {
            return Err("二进制属性的长度无效。".into());
        }
        Ok(slice::from_raw_parts(b.pBlobData, b.cbSize as usize).to_vec())
    }
}

fn open(e: &Endpoint, mode: STGM) -> Result<IPropertyStore> {
    unsafe {
        store::guid(&e.id)?;
        let ep = enumerator()?
            .GetDevice(&HSTRING::from(&e.id))
            .map_err(err)?;
        if !id(&ep)?.eq_ignore_ascii_case(&e.id)
            || ep.GetState().map_err(err)? != DEVICE_STATE_ACTIVE
        {
            return Err("所选播放端点已离线。".into());
        }
        let store = ep.OpenPropertyStore(mode).map_err(err)?;
        let c = store.GetValue(&CONTAINER).map_err(err)?;
        if c.vt() != VT_CLSID
            || !format!("{:?}", PropVariantToGUID(&c).map_err(err)?)
                .eq_ignore_ascii_case(e.container.trim_matches(['{', '}']))
        {
            return Err("端点属性库与所选 USB 容器不一致。".into());
        }
        if blob(&store, &PKEY_AudioEngine_DeviceFormat)? != capability::hex(&e.raw_format)? {
            return Err("所选输出的设备格式已经变化。".into());
        }
        Ok(store)
    }
}

pub(super) fn read_payload(e: &Endpoint) -> Result<Vec<u8>> {
    blob(&open(e, STGM_READ)?, &PID2)
}

pub(super) fn read(e: &Endpoint) -> Result<(Vec<u8>, Vec<u8>)> {
    let bytes = read_payload(e)?;
    let registry = store::persisted(&e.id)?;
    capability::verify_persisted(&bytes, &registry)?;
    Ok((bytes, registry))
}

pub(super) fn verify_preimage(
    before: &[u8],
    before_registry: &[u8],
    now: &[u8],
    now_registry: &[u8],
) -> Result<()> {
    if before != now || before_registry != now_registry {
        return Err("本次操作开始后，音频属性或注册表原件已变化。".into());
    }
    Ok(())
}

pub(super) fn write(
    e: &Endpoint,
    before: &[u8],
    before_registry: &[u8],
    desired: &[u8],
) -> Result<()> {
    let precheck = |err: String| format!("本次未提交端点属性，已保留快照。{err}");
    if before == desired {
        // A previous operation may already have committed and only need a reload.
        let (now, registry) = read(e).map_err(precheck)?;
        return verify_preimage(before, before_registry, &now, &registry).map_err(precheck);
    }
    {
        let writer = open(e, STGM_READWRITE).map_err(precheck)?;
        let now = blob(&writer, &PID2).map_err(precheck)?;
        let registry = store::persisted(&e.id).map_err(precheck)?;
        verify_preimage(before, before_registry, &now, &registry).map_err(precheck)?;
        // SetValue copies the VT_BLOB. Borrow the slice for this synchronous
        // call, so PropVariantClear must never free its backing Vec.
        let v = ManuallyDrop::new(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: desired.len() as u32,
                            pBlobData: desired.as_ptr().cast_mut(),
                        },
                    },
                    ..Default::default()
                }),
            },
        });
        unsafe { writer.SetValue(&PID2, &*v).and_then(|()| writer.Commit()) }
            .map_err(|e| format!("提交端点属性失败，快照仍保留。{}", err(e)))?;
    }
    let (actual, _) =
        read(e).map_err(|err| format!("端点属性已提交，读回未通过。已保留快照。{err}"))?;
    if actual != desired {
        return Err("端点属性已提交，读回的能力正文与目标不符。已保留快照。".into());
    }
    Ok(())
}

#[derive(PartialEq, serde::Serialize)]
pub(super) struct AudioSettings {
    roles: [Option<String>; 3],
    level: Option<(i32, bool)>,
}
pub(super) fn settings(e: &Endpoint) -> Result<AudioSettings> {
    unsafe {
        let en = enumerator()?;
        let roles = [eConsole, eMultimedia, eCommunications].map(|role| {
            en.GetDefaultAudioEndpoint(eRender, role)
                .ok()
                .and_then(|ep| id(&ep).ok())
                .map(|v| v.to_ascii_lowercase())
        });
        let vol = en
            .GetDevice(&HSTRING::from(&e.id))
            .ok()
            .and_then(|ep| ep.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok());
        let level = vol.and_then(|v| {
            Some((
                (v.GetMasterVolumeLevelScalar().ok()? * 10000.0).round() as i32,
                v.GetMute().ok()?.as_bool(),
            ))
        });
        Ok(AudioSettings { roles, level })
    }
}
impl AudioSettings {
    #[cfg(test)]
    pub fn level(&self) -> Result<(i32, bool)> {
        self.level.ok_or("缺少基线音量和静音状态。".into())
    }
    pub fn report(&self, after: &Self, id_before: &str, id_after: &str) -> String {
        let mut lines = Vec::new();
        for (i, name) in ["常规", "多媒体", "通信"].iter().enumerate() {
            let old = self.roles[i].as_deref();
            let now = after.roles[i].as_deref();
            if let Some(now) = now {
                let label = if now.eq_ignore_ascii_case(id_after) {
                    "所选输出"
                } else {
                    now
                };
                let changed = old.is_some_and(|old| {
                    !old.eq_ignore_ascii_case(now)
                        && !(old.eq_ignore_ascii_case(id_before)
                            && now.eq_ignore_ascii_case(id_after))
                });
                lines.push(format!(
                    "{name} {label}{}",
                    if changed { "（已切换）" } else { "" }
                ));
            } else {
                lines.push(format!("{name} 未读到"))
            }
        }
        let volume = after.level.map_or_else(
            || "未读到".to_string(),
            |(l, m)| {
                format!(
                    "{:.1}%{}{}",
                    l as f64 / 100.0,
                    if m { "，静音" } else { "" },
                    if self.level != after.level {
                        "（与操作前不同）"
                    } else {
                        ""
                    }
                )
            },
        );
        format!(
            "默认播放设备：{}。所选输出音量：{volume}。",
            lines.join(" / ")
        )
    }
}

#[cfg(test)]
pub(super) fn check_test_volume(e: &Endpoint, expected: (i32, bool)) -> Result<()> {
    if settings(e)?.level()? != expected {
        return Err("音量或静音状态已发生变化，停止验收。".into());
    }
    Ok(())
}
fn locate(id: &str) -> Result<u32> {
    let mut node = 0;
    let code =
        unsafe { CM_Locate_DevNodeW(&mut node, &HSTRING::from(id), CM_LOCATE_DEVNODE_NORMAL) };
    if code != CR_SUCCESS {
        return Err(format!("无法定位设备实例（CM {}）。", code.0));
    }
    Ok(node)
}
pub(super) fn started(id: &str) -> Result<()> {
    let node = locate(id)?;
    let mut status = CM_DEVNODE_STATUS_FLAGS::default();
    let mut problem = CM_PROB::default();
    let code = unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, node, 0) };
    if code != CR_SUCCESS || status.0 & 8 == 0 || status.0 & 1024 != 0 || problem.0 != 0 {
        return Err(format!(
            "设备尚未启动（CM {}，问题码 {}）。",
            code.0, problem.0
        ));
    }
    Ok(())
}
pub(super) fn arrival(id: &str) -> Result<u64> {
    let mut value = [0; 8];
    let mut len = 8;
    let mut ty = DEVPROPTYPE::default();
    let code = unsafe {
        CM_Get_DevNode_PropertyW(
            locate(id)?,
            &DEVPKEY_Device_LastArrivalDate,
            &mut ty,
            Some(value.as_mut_ptr()),
            &mut len,
            0,
        )
    };
    if code != CR_SUCCESS || ty != DEVPROP_TYPE_FILETIME || len != 8 || value == [0; 8] {
        return Err("无法核实本次物理 USB 连接。".into());
    }
    Ok(u64::from_le_bytes(value))
}

pub(super) fn is_usb_id(id: &str) -> bool {
    let parts: Vec<_> = id.split('\\').collect();
    parts.len() == 3
        && parts[0].eq_ignore_ascii_case("USB")
        && parts[1].to_ascii_uppercase().starts_with("VID_")
        && parts[1].to_ascii_uppercase().contains("&PID_")
        && !parts[2].is_empty()
        && !id
            .bytes()
            .any(|b| b < 32 || matches!(b, b'*' | b'?' | b'"' | b'/'))
}
pub(super) fn scoped_command(args: &[&str]) -> Result<u32> {
    let mut buf = vec![0; 32768];
    let len = unsafe { GetSystemDirectoryW(Some(&mut buf)) } as usize;
    if !(1..buf.len()).contains(&len) {
        return Err("无法确定 Windows 系统目录。".into());
    }
    let dir = PathBuf::from(String::from_utf16(&buf[..len]).map_err(|e| e.to_string())?);
    let result = Command::new(dir.join("pnputil.exe"))
        .current_dir(&dir)
        .args(args)
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("无法启动系统设备管理工具：{e}"))?;
    result
        .status
        .code()
        .map(|n| n as u32)
        .ok_or("设备管理工具异常退出。".into())
}
