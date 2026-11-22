#![windows_subsystem = "windows"]
mod desktop;
mod emergency;
mod preferences;

use uac2_control::{bench_worker, devices};
use windows::{
    Win32::{
        Foundation::ERROR_CANCELLED,
        System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        },
        UI::{
            Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW},
            WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW, SW_SHOWNORMAL},
        },
    },
    core::{HRESULT, HSTRING, PCWSTR, w},
};

fn require_admin() -> Result<(), String> {
    if bench_worker::current_process_elevated()? {
        Ok(())
    } else {
        Err("请使用 Windows 管理员终端运行此命令。".into())
    }
}

fn open_desktop(after_uac: bool) -> Result<(), String> {
    if bench_worker::current_process_elevated()? {
        return desktop::run();
    }
    if after_uac {
        return Err("Windows 未授予管理员权限，无法打开设备控制窗口。".into());
    }
    let path = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = HSTRING::from(path.as_os_str());
    // Only the fixed desktop mode goes through UAC. No caller-supplied paths or
    // arguments are passed to the elevated instance.
    let mut launch = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: w!("--desktop"),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
            .ok()
            .map_err(|e| format!("Windows 无法初始化启动程序（{e}）。"))?;
        let result = ShellExecuteExW(&mut launch);
        CoUninitialize();
        match result {
            Ok(()) => Ok(()),
            Err(e) if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => Ok(()),
            Err(e) => Err(format!(
                "Windows 无法以管理员权限启动 UAC2 Control（{e}）。"
            )),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--benchmark") => std::process::exit(bench_worker::worker_main()),
        Some("--inspect") => match require_admin().and_then(|_| devices::list()) {
            Ok(items) => println!("{}", serde_json::to_string(&items).unwrap()),
            Err(e) => {
                println!("{}", serde_json::json!({"error":e}));
                std::process::exit(1)
            }
        },
        Some("--measure") if args.len() == 3 => {
            let outcome = (|| -> Result<serde_json::Value, String> {
                require_admin()?;
                if !preferences::Preferences::load()?.accepted {
                    return Err("请先在主窗口阅读并接受首次使用警告。".into());
                }
                let device = devices::get(&args[1])?;
                if !device.can_benchmark {
                    return Err("此播放输出未提供可核对的 WASAPI 共享格式和周期，无法试流。".into());
                }
                let dir = uac2_control::benchmark::validate_dir(std::path::Path::new(&args[2]))?;
                uac2_control::bench_worker::measure(&device, &dir, |p| eprintln!("{p}"), || false)
            })();
            match outcome {
                Ok(data) => println!("{data}"),
                Err(e) => {
                    println!("{}", serde_json::json!({"error":e}));
                    std::process::exit(1)
                }
            }
        }
        None | Some("--desktop") if args.len() <= 1 => {
            if let Err(e) = open_desktop(!args.is_empty()) {
                unsafe {
                    MessageBoxW(
                        None,
                        &HSTRING::from(e),
                        w!("UAC2 Control"),
                        MB_OK | MB_ICONERROR,
                    );
                }
                std::process::exit(1);
            }
        }
        _ => std::process::exit(2),
    }
}
