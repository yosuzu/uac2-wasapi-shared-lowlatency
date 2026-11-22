//! Authenticated, local-only IPC between the control window and its ordinary-user audio helper.
use crate::{benchmark, devices::Endpoint};
use serde_json::{Value, json};
use std::ffi::{OsStr, OsString, c_void};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{
    CloseHandle, DUPLICATE_HANDLE_OPTIONS, DuplicateHandle, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Security::{
    ACE_HEADER, ACL, GetAce, GetTokenInformation, IsWellKnownSid, LABEL_SECURITY_INFORMATION, PSID,
    SYSTEM_MANDATORY_LABEL_ACE, TOKEN_ACCESS_MASK, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    WinHighLabelSid, WinSystemLabelSid,
};
use windows::Win32::Storage::FileSystem::READ_CONTROL;
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateGuid, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IDispatch, IServiceProvider,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows, SID_STopLevelBrowser,
    SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
};

use windows::Win32::System::Threading::*;

use windows::core::{BSTR, Interface, PWSTR};

const MAX_MESSAGE: usize = 64 * 1024;
const TICK: Duration = Duration::from_millis(20);
const STARTUP: Duration = Duration::from_secs(10);
const IDLE: Duration = Duration::from_secs(45);
const CANCEL_GRACE: Duration = Duration::from_secs(12);
const TOTAL_LIMIT: Duration = Duration::from_secs(30 * benchmark::TOTAL as u64);
type Raw = *mut c_void;

#[repr(C)]
struct Attributes {
    size: u32,
    descriptor: Raw,
    inherit: i32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetErrorMode() -> u32;
    fn SetErrorMode(mode: u32) -> u32;
    fn CreateNamedPipeW(
        name: *const u16,
        access: u32,
        mode: u32,
        instances: u32,
        out_size: u32,
        in_size: u32,
        timeout: u32,
        security: *const Attributes,
    ) -> Raw;
    fn ConnectNamedPipe(pipe: Raw, overlapped: Raw) -> i32;
    fn DisconnectNamedPipe(pipe: Raw) -> i32;
    fn GetNamedPipeClientProcessId(pipe: Raw, id: *mut u32) -> i32;
    fn GetNamedPipeServerProcessId(pipe: Raw, id: *mut u32) -> i32;
    fn PeekNamedPipe(
        pipe: Raw,
        buffer: Raw,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        left: *mut u32,
    ) -> i32;
    fn SetNamedPipeHandleState(
        pipe: Raw,
        mode: *const u32,
        max_count: *const u32,
        timeout: *const u32,
    ) -> i32;
    fn LocalFree(memory: Raw) -> Raw;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn GetSecurityInfo(
        handle: Raw,
        object_type: u32,
        information: u32,
        owner: *mut Raw,
        group: *mut Raw,
        dacl: *mut *mut ACL,
        sacl: *mut *mut ACL,
        descriptor: *mut Raw,
    ) -> u32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        input: *const u16,
        revision: u32,
        output: *mut Raw,
        size: *mut u32,
    ) -> i32;
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}
fn native_error(context: &str) -> String {
    format!("{context}（{}）。", io::Error::last_os_error())
}
fn win<T>(r: windows::core::Result<T>, context: &str) -> Result<T, String> {
    r.map_err(|e| format!("{context}（{e}）。"))
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn token(process: HANDLE, rights: TOKEN_ACCESS_MASK) -> Result<Handle, String> {
    let mut h = HANDLE::default();
    win(
        unsafe { OpenProcessToken(process, rights, &mut h) },
        "无法读取本机用户令牌",
    )?;
    Ok(Handle(h))
}
fn elevated(token: &Handle) -> Result<bool, String> {
    let mut state = TOKEN_ELEVATION::default();
    let mut size = 0;
    win(
        unsafe {
            GetTokenInformation(
                token.0,
                TokenElevation,
                Some((&raw mut state).cast()),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
        },
        "无法确认试流权限",
    )?;
    Ok(state.TokenIsElevated != 0)
}
pub fn current_process_elevated() -> Result<bool, String> {
    elevated(&token(unsafe { GetCurrentProcess() }, TOKEN_QUERY)?)
}

fn elevated_server(process: HANDLE) -> Result<bool, String> {
    if let Ok(primary) = token(process, TOKEN_QUERY) {
        return elevated(&primary);
    }
    // The desktop user can be different from the elevated user. In that case the token
    // DACL can prevent opening it. Read the kernel's label on this same process object.
    const SE_KERNEL_OBJECT: u32 = 6;
    const MANDATORY_LABEL_ACE_TYPE: u8 = 0x11;
    let mut sacl: *mut ACL = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            process.0,
            SE_KERNEL_OBJECT,
            LABEL_SECURITY_INFORMATION.0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut sacl,
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(format!("无法核对主程序完整性级别（Windows {status}）。"));
    }
    let mut high = false;
    if !sacl.is_null() {
        for index in 0..unsafe { (*sacl).AceCount } {
            let mut ace = ptr::null_mut();
            if unsafe { GetAce(sacl, u32::from(index), &mut ace) }.is_ok()
                && unsafe { (*ace.cast::<ACE_HEADER>()).AceType } == MANDATORY_LABEL_ACE_TYPE
            {
                let sid = PSID(
                    unsafe { &raw mut (*ace.cast::<SYSTEM_MANDATORY_LABEL_ACE>()).SidStart }.cast(),
                );
                high = unsafe {
                    IsWellKnownSid(sid, WinHighLabelSid).as_bool()
                        || IsWellKnownSid(sid, WinSystemLabelSid).as_bool()
                };
                break;
            }
        }
    }
    unsafe { LocalFree(descriptor) };
    Ok(high)
}
fn process_path(process: HANDLE) -> Result<PathBuf, String> {
    let mut buf = vec![0u16; 32768];
    let mut len = buf.len() as u32;
    win(
        unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_FORMAT(0),
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
        },
        "无法核对试流程序路径",
    )?;
    PathBuf::from(OsString::from_wide(&buf[..len as usize]))
        .canonicalize()
        .map_err(|e| e.to_string())
}

struct Child {
    process: Handle,
}
impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            if WaitForSingleObject(self.process.0, 750) != WAIT_OBJECT_0 {
                // This handle came from our own fixed-path launch. No device state was written.
                let _ = TerminateProcess(self.process.0, 3);
                WaitForSingleObject(self.process.0, 2000);
            }
        }
    }
}

// Ask the already-running desktop Explorer to perform the fixed "open" action, as in
// Microsoft's ExecInExplorer sample. The desktop shell supplies its own user and environment.
fn launch(path: &Path, key: &str) -> Result<(), String> {
    let path = path.to_str().ok_or("试流程序路径包含无效的 Unicode。")?;
    let image = if let Some(remote) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{remote}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
    };
    let args = format!("--benchmark {} {}", key, std::process::id());
    let folder = Path::new(&image)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or("")
        .to_owned();
    desktop_open(image, args, folder, false)
        .map_err(|e| format!("当前 Windows 桌面无法启动普通用户试流子程序（{e}）。"))
}

/// Only these two fixed HTTPS destinations can be opened by the desktop UI.
pub fn open_project_link(issue: bool) -> Result<(), String> {
    let url = if issue {
        "https://github.com/yosuzu/uac2-wasapi-shared-lowlatency/issues/new"
    } else {
        "https://github.com/yosuzu/uac2-wasapi-shared-lowlatency"
    };
    desktop_open(url.into(), String::new(), String::new(), true)
}

fn desktop_open(
    image: String,
    args: String,
    folder: String,
    browser_link: bool,
) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let call = || -> windows::core::Result<()> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
                struct Apartment;
                impl Drop for Apartment {
                    fn drop(&mut self) {
                        unsafe { CoUninitialize() }
                    }
                }
                let _apartment = Apartment;
                let windows: IShellWindows =
                    CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
                let empty = VARIANT::default();
                let desk =
                    windows.FindWindowSW(&empty, &empty, SWC_DESKTOP, &mut 0, SWFO_NEEDDISPATCH)?;
                let browser: IShellBrowser = desk
                    .cast::<IServiceProvider>()?
                    .QueryService(&SID_STopLevelBrowser)?;
                if browser_link {
                    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
                    let mut pid = 0;
                    GetWindowThreadProcessId(browser.GetWindow()?, Some(&mut pid));
                    let shell_process =
                        Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?);
                    let ordinary =
                        token(shell_process.0, TOKEN_QUERY).and_then(|t| elevated(&t)) == Ok(false);
                    if !ordinary {
                        return Err(windows::core::Error::from(
                            windows::Win32::Foundation::E_ACCESSDENIED,
                        ));
                    }
                }
                let background: IDispatch = browser
                    .QueryActiveShellView()?
                    .GetItemObject(SVGIO_BACKGROUND)?;
                let shell = background
                    .cast::<IShellFolderViewDual>()?
                    .Application()?
                    .cast::<IShellDispatch2>()?;
                shell.ShellExecute(
                    &BSTR::from(image.as_str()),
                    &VARIANT::from(args.as_str()),
                    &VARIANT::from(folder.as_str()),
                    &VARIANT::from("open"),
                    &VARIANT::from(if browser_link { 1i32 } else { 0i32 }),
                )
            }
        };
        let _ = tx.send(call().map_err(|e| e.to_string()));
    });
    rx.recv_timeout(STARTUP)
        .map_err(|_| "Windows desktop timed out.".to_owned())?
}
fn pipe_name(key: &str) -> Vec<u16> {
    wide(OsStr::new(&format!(r"\\.\pipe\UAC2-Audio-{key}")))
}
fn make_pipe(key: &str) -> Result<File, String> {
    // Interactive users can connect across UAC or credential elevation. The first-instance
    // flag, one instance, local-only mode and subsequent client PID check bind this launch.
    let policy = wide(OsStr::new(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)S:(ML;;NW;;;ME)",
    ));
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            policy.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(native_error("无法限制本机试流通道权限"));
    }
    let attrs = Attributes {
        size: size_of::<Attributes>() as u32,
        descriptor,
        inherit: 0,
    };
    let raw = unsafe {
        CreateNamedPipeW(
            pipe_name(key).as_ptr(),
            0x80000 | 3,
            0x8 | 1,
            1,
            MAX_MESSAGE as u32 * 2,
            MAX_MESSAGE as u32 * 2,
            0,
            &attrs,
        )
    };
    let error = native_error("无法建立本机试流通道");
    unsafe { LocalFree(descriptor) };
    if raw as isize == -1 {
        return Err(error);
    }
    Ok(unsafe { File::from_raw_handle(raw) })
}

fn available(pipe: &File) -> Result<usize, String> {
    let mut count = 0;
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut count,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(native_error("试流通道已断开"));
    }
    Ok(count as usize)
}

fn send(mut pipe: &File, message: &Value) -> Result<(), String> {
    let mut frame = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    if frame.len() > MAX_MESSAGE {
        return Err("试流消息过长。".into());
    }
    frame.push(b'\n');
    let mut offset = 0;
    let until = Instant::now() + Duration::from_secs(2);
    while offset < frame.len() {
        if Instant::now() >= until {
            return Err("试流通道写入超时。".into());
        }
        match pipe.write(&frame[offset..]) {
            Ok(0) => sleep(TICK),
            Ok(n) => offset += n,
            Err(e) => return Err(format!("试流消息发送失败（{e}）。")),
        }
    }
    Ok(())
}

#[derive(Default)]
struct Reader {
    buffer: Vec<u8>,
}
impl Reader {
    fn next(&mut self, mut pipe: &File) -> Result<Option<Value>, String> {
        if !self.buffer.contains(&b'\n') {
            let mut chunk = [0u8; 4096];
            let n = available(pipe)?.min(chunk.len());
            if n > 0 {
                let n = pipe.read(&mut chunk[..n]).map_err(|e| e.to_string())?;
                self.buffer.extend_from_slice(&chunk[..n]);
            }
        }
        if let Some(end) = self.buffer.iter().position(|&b| b == b'\n') {
            if end > MAX_MESSAGE {
                return Err("试流消息过长。".into());
            }
            let value = serde_json::from_slice(&self.buffer[..end])
                .map_err(|e| format!("试流消息无效（{e}）。"))?;
            self.buffer.drain(..=end);
            return Ok(Some(value));
        }
        if self.buffer.len() > MAX_MESSAGE {
            return Err("试流消息过长。".into());
        }
        Ok(None)
    }
}

fn connect(pipe: &File, expected: &Path, cancelled: &impl Fn() -> bool) -> Result<Child, String> {
    let raw = pipe.as_raw_handle();
    let start = Instant::now();
    loop {
        if cancelled() {
            return Err("试流已取消。".into());
        }

        if start.elapsed() > STARTUP {
            return Err("普通用户试流子程序连接超时。".into());
        }
        let connected = unsafe { ConnectNamedPipe(raw, ptr::null_mut()) };
        let code = io::Error::last_os_error().raw_os_error();
        if connected != 0 || code == Some(535) {
            let mut pid = 0;
            if unsafe { GetNamedPipeClientProcessId(raw, &mut pid) } != 0 {
                let rights = PROCESS_QUERY_LIMITED_INFORMATION
                    | PROCESS_SYNCHRONIZE
                    | PROCESS_TERMINATE
                    | PROCESS_DUP_HANDLE;
                if let Ok(p) = unsafe { OpenProcess(rights, false, pid) } {
                    let process = Handle(p);
                    if unsafe { GetProcessId(p) } == pid
                        && process_path(p).is_ok_and(|path| path == expected)
                        && token(p, TOKEN_QUERY).and_then(|t| elevated(&t)) == Ok(false)
                    {
                        return Ok(Child { process });
                    }
                }
            }
            unsafe { DisconnectNamedPipe(raw) };
        } else if code == Some(232) {
            unsafe { DisconnectNamedPipe(raw) };
        } else if code != Some(536) {
            return Err(format!("无法连接试流通道（Windows {code:?}）。"));
        }
        sleep(TICK);
    }
}

pub fn measure(
    expected: &Endpoint,
    dir: &Path,
    progress: impl FnMut(Value),
    cancelled: impl Fn() -> bool,
) -> Result<Value, String> {
    if !current_process_elevated()? {
        return Err("试流须由管理员主程序发起。".into());
    }
    let session = exchange(expected, dir, progress, cancelled);
    // The worker and pipe have now been closed, including abnormal termination.
    let condition = benchmark::verify_condition(expected);
    match (session, condition) {
        (Ok(mut value), Err(e)) => {
            value["restored"] = json!(false);
            value["restoreReason"] = json!(e);
            value["defaultDelta"] = Value::Null;
            value["sevenDelta"] = Value::Null;
            value["defaultComparison"] = json!("unavailable");
            value["sevenComparison"] = json!("unavailable");
            Ok(value)
        }
        (Ok(value), Ok(_)) => Ok(value),
        (Err(e), Ok(_)) => Err(format!("{e} 已复核所选输出，格式与公布周期设置一致。")),
        (Err(e), Err(reason)) => Err(format!("{e} 设置待复核：{reason}")),
    }
}

fn exchange(
    expected: &Endpoint,
    dir: &Path,
    mut progress: impl FnMut(Value),
    cancelled: impl Fn() -> bool,
) -> Result<Value, String> {
    let key = format!(
        "{:032x}",
        win(unsafe { CoCreateGuid() }, "无法创建试流会话")?.to_u128()
    );
    let pipe = make_pipe(&key)?;
    let expected_image = process_path(unsafe { GetCurrentProcess() })?;
    launch(&expected_image, &key)?;
    let child = connect(&pipe, &expected_image, &cancelled)?;
    // A standard desktop account may have used a different administrator's credentials.
    // Give the authenticated child only the rights to read the server identity and label.
    let mut query_handle = HANDLE::default();
    unsafe {
        let own = GetCurrentProcess();
        win(
            DuplicateHandle(
                own,
                own,
                child.process.0,
                &mut query_handle,
                PROCESS_QUERY_LIMITED_INFORMATION.0 | READ_CONTROL.0,
                false,
                DUPLICATE_HANDLE_OPTIONS(0),
            ),
            "无法提供主程序身份核对",
        )?;
    }
    send(&pipe, &json!([expected, dir, query_handle.0 as usize]))?;
    let start = Instant::now();
    let mut last = start;
    let mut cancel_sent = None;
    let mut reader = Reader::default();
    let mut count = 0;
    loop {
        if cancelled() && cancel_sent.is_none() {
            send(&pipe, &json!("cancel"))?;
            cancel_sent = Some(Instant::now());
        }
        if start.elapsed() > TOTAL_LIMIT
            || last.elapsed() > IDLE
            || cancel_sent.is_some_and(|t| t.elapsed() > CANCEL_GRACE)
        {
            return Err("试流子程序无应答，已结束该子程序。".into());
        }
        match reader.next(&pipe) {
            Ok(Some(frame)) => {
                count += 1;
                if count > 128 {
                    return Err("试流子程序发送消息过多。".into());
                }
                last = Instant::now();
                match frame["type"].as_str() {
                    Some("progress") => progress(frame["data"].clone()),
                    Some("result") if frame["data"].is_object() => {
                        if unsafe { WaitForSingleObject(child.process.0, 2000) } != WAIT_OBJECT_0 {
                            return Err("试流子程序未正常结束。".into());
                        }
                        let mut code = 0;
                        win(
                            unsafe { GetExitCodeProcess(child.process.0, &mut code) },
                            "无法确认试流子程序退出状态",
                        )?;
                        if code != 0 {
                            return Err(format!("试流子程序异常退出（{code}）。"));
                        }
                        return Ok(frame["data"].clone());
                    }
                    Some("error") => {
                        return Err(frame["message"].as_str().unwrap_or("试流失败。").into());
                    }
                    _ => return Err("试流子程序返回无法识别的消息。".into()),
                }
            }
            Ok(None) => sleep(TICK),
            Err(e) => return Err(e),
        }
    }
}

fn worker() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3
        || args[0] != "--benchmark"
        || args[1].len() != 32
        || !args[1].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("试流子程序须由控制窗口启动。".into());
    }
    if current_process_elevated()? {
        return Err("试流需要普通用户进程。".into());
    }
    let parent_id = args[2].parse::<u32>().map_err(|e| e.to_string())?;
    let name = pipe_name(&args[1]);
    let path = OsString::from_wide(&name[..name.len() - 1]);
    let pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(0x100000 | 0x10000)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut server_id = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut server_id) } == 0
        || server_id != parent_id
    {
        return Err("试流主进程身份不符。".into());
    }
    let nowait = 1;
    if unsafe { SetNamedPipeHandleState(pipe.as_raw_handle(), &nowait, ptr::null(), ptr::null()) }
        == 0
    {
        return Err(native_error("无法设置试流通道"));
    }
    let mut reader = Reader::default();
    let start = Instant::now();
    let request = loop {
        if let Some(v) = reader.next(&pipe)? {
            break v;
        }
        if start.elapsed() > STARTUP {
            return Err("等待试流请求超时。".into());
        }
        sleep(TICK);
    };
    let result = (|| {
        let (expected, dir, query): (Endpoint, PathBuf, usize) =
            serde_json::from_value(request).map_err(|e| e.to_string())?;
        if query == 0 || query > u32::MAX as usize {
            return Err("试流主程序句柄无效。".into());
        }
        let server = Handle(HANDLE(query as Raw));
        if unsafe { GetProcessId(server.0) } != server_id
            || process_path(server.0)? != process_path(unsafe { GetCurrentProcess() })?
        {
            return Err("试流主程序路径或进程不符。".into());
        }
        if !elevated_server(server.0)? {
            return Err("试流须由管理员主程序发起。".into());
        }
        drop(server);
        // Each progress message and the final result travel only back to the authenticated server.
        benchmark::run(
            &expected,
            &dir,
            |v| {
                let _ = send(&pipe, &json!({"type":"progress","data":v}));
            },
            || !reader.buffer.is_empty() || available(&pipe).map_or(true, |n| n > 0),
        )
    })();
    let response = match result {
        Ok(v) => json!({"type":"result","data":v}),
        Err(e) => json!({"type":"error","message":e}),
    };
    send(&pipe, &response)
}

pub fn worker_main() -> i32 {
    // Return loader/drive errors through our IPC instead of interactive Windows error boxes.
    unsafe {
        SetErrorMode(GetErrorMode() | 0x0001 | 0x0002 | 0x8000);
    }
    if worker().is_ok() { 0 } else { 1 }
}
