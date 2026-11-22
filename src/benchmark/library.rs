mod trust;
use std::ffi::{CStr, OsStr, c_char, c_void};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

const NAMES: [&str; 2] = ["bass.dll", "basswasapi.dll"];
// Un4seen's BASSVERSION check uses HIWORD for the API generation; LOWORD is
// the compatible revision. All revisions share this same FFI declaration.
const BASS_API_GENERATION:u32=0x0204;
const SYSTEM32: u32 = 0x800;
type Handle = *mut c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetDriveTypeW(path: *const u16) -> u32;
    fn SetDefaultDllDirectories(flags: u32) -> i32;
    fn LoadLibraryExW(path: *const u16, file: Handle, flags: u32) -> Handle;
    fn GetProcAddress(module: Handle, name: *const c_char) -> *const c_void;
    fn FreeLibrary(module: Handle) -> i32;
    fn GetCurrentProcess() -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn GetTokenInformation(
        token: Handle,
        class: u32,
        info: Handle,
        size: u32,
        used: *mut u32,
    ) -> i32;
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn require_local(path: &Path) -> Result<(), String> {
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return Err("请选择本机磁盘上的绝对路径。".into());
    };
    let letter = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) if path.has_root() => letter,
        _ => return Err("请选择本机磁盘上的游戏目录。".into()),
    };
    let root = [u16::from(letter), b':' as u16, b'\\' as u16, 0];
    match unsafe { GetDriveTypeW(root.as_ptr()) } {
        2 | 3 | 5 | 6 => Ok(()), // removable, fixed, optical or RAM disk
        _ => Err("游戏目录必须位于可读取的本机磁盘上。".into()),
    }
}

fn verify_pe(file: &mut File) -> Result<(), String> {
    let mut dos = [0; 64];
    let mut header = [0; 26];
    let mut read = || -> io::Result<()> {
        file.read_exact(&mut dos)?;
        let offset = u32::from_le_bytes(dos[60..64].try_into().unwrap());
        file.seek(SeekFrom::Start(u64::from(offset)))?;
        file.read_exact(&mut header)
    };
    read().map_err(|e| format!("Windows 可执行文件头读取失败（{e}）"))?;
    if &dos[..2] != b"MZ" || &header[..4] != b"PE\0\0" {
        return Err("不是有效的 Windows PE 文件".into());
    }
    let machine = u16::from_le_bytes([header[4], header[5]]);
    let optional = u16::from_le_bytes([header[24], header[25]]);
    if machine != 0x8664 || optional != 0x20b {
        return Err(format!("需要 x64，实际机器类型为 0x{machine:04X}"));
    }
    if u16::from_le_bytes([header[22], header[23]]) & 0x2000 == 0 {
        return Err("PE 文件未标记为 DLL".into());
    }
    Ok(())
}

fn open_files(path: &Path) -> Result<(PathBuf, Vec<File>), String> {
    require_local(path)?;
    let entry = fs::metadata(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => "所选目录不存在。".to_owned(),
        io::ErrorKind::PermissionDenied => format!("无权读取所选目录（{e}）。"),
        _ => format!("无法读取所选目录（{e}）。"),
    })?;
    if !entry.is_dir() {
        return Err("所选路径不是目录。".into());
    }
    let mut dir = path.to_owned();
    if !path
        .file_name()
        .is_some_and(|s| s.eq_ignore_ascii_case("current"))
    {
        let current = path.join("current");
        match fs::metadata(&current) {
            Ok(m) if m.is_dir() => dir = current,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("无法读取 current 目录（{e}）。")),
        }
    }
    let dir = dir
        .canonicalize()
        .map_err(|e| format!("无法打开目录（{e}）。"))?;
    require_local(&dir)?;
    fs::read_dir(&dir).map_err(|e| format!("无法列出所选目录，检查读取权限（{e}）。"))?;
    let mut files = Vec::new();
    // Keep the verified path stable while Windows' loader opens the named files.
    for ancestor in dir.ancestors() {
        let folder = File::options()
            .access_mode(0x80)
            .share_mode(3)
            .custom_flags(0x02000000)
            .open(ancestor)
            .map_err(|e| format!("无法只读锁定音频库目录（{e}）。"))?;
        files.push(folder);
    }
    let mut issues = Vec::new();
    for name in NAMES {
        let file_path = dir.join(name);
        let resolved = match file_path.canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                issues.push(format!("缺少 {name}"));
                continue;
            }
            Err(e) => {
                issues.push(format!("无法读取 {name}（{e}）"));
                continue;
            }
        };
        if resolved.parent() != Some(&dir)
            || !resolved
                .file_name()
                .is_some_and(|s| s.eq_ignore_ascii_case(name))
        {
            issues.push(format!("{name} 指向所选目录外部"));
            continue;
        }
        match File::options().read(true).share_mode(1).open(&resolved) {
            Ok(mut f) => match verify_pe(&mut f).and_then(|()| trust::publisher(&f, &resolved)) {
                Ok(()) => files.push(f),
                Err(e) => issues.push(format!("{name}：{e}")),
            },
            Err(e) => issues.push(format!("无法读取 {name}（{e}）")),
        }
    }
    if !issues.is_empty() {
        return Err(issues.join("；"));
    }
    Ok((dir, files))
}

pub(super) fn validate_dir(path: &Path) -> Result<PathBuf, String> {
    open_files(path).map(|(dir, _)| dir)
}

fn require_worker() -> Result<(), String> {
    if std::env::args_os().nth(1).as_deref() != Some(OsStr::new("--benchmark")) {
        return Err("试流须在独立的普通用户测试进程中运行。".into());
    }
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), 8, &mut token) == 0 {
            return Err(format!(
                "无法核对试流进程权限（{}）。",
                io::Error::last_os_error()
            ));
        }
        let mut elevated = 0u32;
        let mut used = 0;
        let ok = GetTokenInformation(token, 20, (&raw mut elevated).cast(), 4, &mut used);
        let error = io::Error::last_os_error();
        CloseHandle(token);
        if ok == 0 || used != 4 {
            return Err(format!("无法核对试流进程权限（{error}）。"));
        }
        if elevated != 0 {
            return Err("试流进程已提升权限。请以普通用户身份启动本工具。".into());
        }
    }
    Ok(())
}

struct Module {
    handle: Handle,
    name: &'static str,
}

impl Module {
    fn load(dir: &Path, name: &'static str) -> Result<Self, String> {
        let path = wide(dir.join(name).as_os_str());
        let handle = unsafe { LoadLibraryExW(path.as_ptr(), ptr::null_mut(), SYSTEM32) };
        if handle.is_null() {
            return Err(format!(
                "{name} 加载失败（{}）。",
                io::Error::last_os_error()
            ));
        }
        Ok(Self { handle, name })
    }
    fn symbol(&self, name: &CStr) -> Result<*const c_void, String> {
        let p = unsafe { GetProcAddress(self.handle, name.as_ptr()) };
        if p.is_null() {
            return Err(format!(
                "{} 缺少接口 {}。",
                self.name,
                name.to_string_lossy()
            ));
        }
        Ok(p)
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.handle) };
    }
}

#[repr(C)]
#[derive(Default)]
struct DeviceInfo {
    name: *const c_char,
    id: *const c_char,
    kind: u32,
    flags: u32,
    min_period: f32,
    def_period: f32,
    mix_freq: u32,
    mix_chans: u32,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct StreamInfo {
    flags: u32,
    pub rate: u32,
    pub channels: u32,
    format: u32,
    pub buffer_bytes: u32,
    volume_max: f32,
    volume_min: f32,
    volume_step: f32,
}

type Callback = unsafe extern "system" fn(Handle, u32, Handle) -> u32;
type WasapiInit = unsafe extern "system" fn(i32, u32, u32, u32, f32, f32, Callback, Handle) -> i32;
type BassInit = unsafe extern "system" fn(i32, u32, u32, Handle, Handle) -> i32;
type NoArgs = unsafe extern "system" fn() -> i32;
type Version = unsafe extern "system" fn() -> u32;

// One benchmark runs in its own process. A callback can only access static counters,
// including while an unsuccessful device release is being handled.
static CALLBACKS: AtomicU32 = AtomicU32::new(0);
static MIN_BYTES: AtomicU32 = AtomicU32::new(u32::MAX);

unsafe extern "system" fn silence(buffer: Handle, length: u32, _: Handle) -> u32 {
    if buffer.is_null() || length == 0 {
        return 0;
    }
    unsafe { ptr::write_bytes(buffer.cast::<u8>(), 0, length as usize) };
    MIN_BYTES.fetch_min(length, Ordering::Relaxed);
    CALLBACKS.fetch_add(1, Ordering::Release);
    length
}

pub(super) fn callback_counts() -> (u32, u32) {
    (
        CALLBACKS.load(Ordering::Acquire),
        MIN_BYTES.load(Ordering::Relaxed),
    )
}

pub(super) fn start_callback_window() -> u32 {
    MIN_BYTES.store(u32::MAX, Ordering::Relaxed);
    CALLBACKS.load(Ordering::Acquire)
}

pub(super) struct Audio {
    // Keep both modules resident until all open native contexts have been released.
    modules: Option<(Module, Module)>,
    error: NoArgs,
    bass_free: NoArgs,
    wasapi_free: NoArgs,
    start: NoArgs,
    init: WasapiInit,
    device_info: unsafe extern "system" fn(u32, *mut DeviceInfo) -> i32,
    stream_info: unsafe extern "system" fn(*mut StreamInfo) -> i32,
    get_data: unsafe extern "system" fn(Handle, u32) -> u32,
    pub versions: [String; 2],
    bass_active: bool,
    stream_active: bool,
}

impl Audio {
    pub fn new(path: &Path, rate: u32) -> Result<Self, String> {
        require_worker()?;
        let (dir, _locked_files) = open_files(path)?;
        if unsafe { SetDefaultDllDirectories(SYSTEM32) } == 0 {
            return Err(format!(
                "无法限定音频库加载目录（{}）。",
                io::Error::last_os_error()
            ));
        }
        let bass = Module::load(&dir, NAMES[0])?;
        let wasapi = Module::load(&dir, NAMES[1])?;
        // Each exported function is converted to its exact ABI type, inferred from its field.
        macro_rules! export {
            ($module:ident, $name:literal) => {
                unsafe { std::mem::transmute($module.symbol($name)?) }
            };
        }
        let bass_version: Version = export!(bass, c"BASS_GetVersion");
        let wasapi_version: Version = export!(wasapi, c"BASS_WASAPI_GetVersion");
        let numbers = unsafe { [bass_version(), wasapi_version()] };
        let versions = numbers.map(|n| {
            let [a, b, c, d] = n.to_be_bytes();
            format!("{a}.{b}.{c}.{d}")
        });
        for (i, version) in numbers.iter().enumerate() {
            if version >> 16 != BASS_API_GENERATION {
                return Err(format!(
                    "{} 报告版本 {}。试流需要 2.4 接口代际，支持该代际所有兼容修订。",
                    NAMES[i], versions[i]
                ));
            }
        }
        let bass_init: BassInit = export!(bass, c"BASS_Init");
        let mut audio = Self {
            error: export!(bass, c"BASS_ErrorGetCode"),
            bass_free: export!(bass, c"BASS_Free"),
            wasapi_free: export!(wasapi, c"BASS_WASAPI_Free"),
            start: export!(wasapi, c"BASS_WASAPI_Start"),
            init: export!(wasapi, c"BASS_WASAPI_Init"),
            device_info: export!(wasapi, c"BASS_WASAPI_GetDeviceInfo"),
            stream_info: export!(wasapi, c"BASS_WASAPI_GetInfo"),
            get_data: export!(wasapi, c"BASS_WASAPI_GetData"),
            versions,
            modules: Some((wasapi, bass)),
            bass_active: false,
            stream_active: false,
        };
        if unsafe { bass_init(0, rate, 0, ptr::null_mut(), ptr::null_mut()) } == 0 {
            return Err(audio.failure("bass.dll 初始化无声设备"));
        }
        audio.bass_active = true;
        Ok(audio)
    }

    fn failure(&self, operation: &str) -> String {
        format!("{operation}失败（BASS 错误 {}）。", unsafe {
            (self.error)()
        })
    }

    pub fn find_output(&self, id: &str) -> Result<i32, String> {
        const ENABLED: u32 = 1;
        const INPUT_OR_LOOPBACK: u32 = 16 | 8;
        for index in 0..i32::MAX {
            let mut info = DeviceInfo::default();
            if unsafe { (self.device_info)(index as u32, &mut info) } == 0 {
                break;
            }
            if info.id.is_null() || info.flags & (ENABLED | INPUT_OR_LOOPBACK) != ENABLED {
                continue;
            }
            let candidate = unsafe { CStr::from_ptr(info.id) }.to_bytes();
            if candidate.eq_ignore_ascii_case(id.as_bytes()) {
                return Ok(index);
            }
        }
        Err("音频库未找到相同 Windows 端点 ID 的活动播放输出。".into())
    }

    pub fn open(&mut self, output: i32, seconds: f32) -> Result<StreamInfo, String> {
        const EVENT_AUTOFORMAT: u32 = 16 | 2;
        CALLBACKS.store(0, Ordering::Relaxed);
        MIN_BYTES.store(u32::MAX, Ordering::Relaxed);
        if unsafe {
            (self.init)(
                output,
                0,
                0,
                EVENT_AUTOFORMAT,
                0.0,
                seconds,
                silence,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(self.failure("打开共享事件静音流"));
        }
        self.stream_active = true;
        let mut info = StreamInfo::default();
        if unsafe { (self.stream_info)(&mut info) } == 0 {
            return Err(self.failure("读取播放流格式"));
        }
        if info.flags & (EVENT_AUTOFORMAT | 1) != EVENT_AUTOFORMAT {
            return Err("音频库未按请求打开共享事件流。".into());
        }
        if unsafe { (self.start)() } == 0 {
            return Err(self.failure("启动静音流"));
        }
        Ok(info)
    }

    pub fn queue_bytes(&self) -> Result<u32, String> {
        let bytes = unsafe { (self.get_data)(ptr::null_mut(), 0) };
        if bytes == u32::MAX {
            return Err(self.failure("读取输出队列"));
        }
        Ok(bytes)
    }

    pub fn close_stream(&mut self) -> Result<(), String> {
        if self.stream_active && unsafe { (self.wasapi_free)() } == 0 {
            return Err(self.failure("释放静音播放流"));
        }
        self.stream_active = false;
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), String> {
        self.close_stream()?;
        if self.bass_active && unsafe { (self.bass_free)() } == 0 {
            return Err(self.failure("释放无声设备"));
        }
        self.bass_active = false;
        Ok(())
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        if self.close().is_err() {
            // An audio thread may still execute vendor code. The OS unloads it at worker exit.
            std::mem::forget(self.modules.take());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn header(machine: u16, optional: u16) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&machine.to_le_bytes());
        bytes[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
        bytes[88..90].copy_from_slice(&optional.to_le_bytes());
        bytes
    }

    #[test]
    fn selected_root_and_current_give_precise_file_diagnostics_without_loading_code() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture =
            std::env::temp_dir().join(format!("uac2-benchmark-pe-{}-{unique}", std::process::id()));
        let current = fixture.join("current");
        fs::create_dir_all(&current).unwrap();
        let bass = current.join(NAMES[0]);
        let wasapi = current.join(NAMES[1]);
        let missing = validate_dir(&fixture).unwrap_err();
        assert!(NAMES.iter().all(|name| missing.contains(name)));
        fs::write(&bass, header(0x8664, 0x20b)).unwrap();
        let missing_one = validate_dir(&fixture).unwrap_err();
        assert!(
            missing_one.contains("缺少 basswasapi.dll") && !missing_one.contains("缺少 bass.dll")
        );
        fs::write(&wasapi, header(0x14c, 0x10b)).unwrap();
        let wrong_arch = validate_dir(&fixture).unwrap_err();
        assert!(wrong_arch.contains(NAMES[1]) && wrong_arch.contains("x64"));
        fs::write(&wasapi, header(0x8664, 0x20b)).unwrap();
        let unsigned = validate_dir(&fixture).unwrap_err();
        assert!(
            unsigned.contains("Windows 签名验证失败") && NAMES.iter().all(|n| unsigned.contains(n))
        );
        assert_eq!(unsigned, validate_dir(&current).unwrap_err());
        // A renamed Windows system image still cannot supply the audio publisher identity.
        let system =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/kernel32.dll");
        fs::copy(system, &bass).unwrap();
        assert!(validate_dir(&current).unwrap_err().contains("bass.dll："));
        fs::remove_file(bass).unwrap();
        fs::remove_file(wasapi).unwrap();
        fs::remove_dir(current).unwrap();
        fs::remove_dir(fixture).unwrap();
    }
}
