use crate::preferences::Preferences;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tao::{
    dpi::{LogicalSize, PhysicalSize},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::windows::{IconExtWindows, WindowBuilderExtWindows, WindowExtWindows},
    window::{Icon, ResizeDirection, WindowBuilder},
};
use uac2_control::{
    bench_worker, benchmark,
    devices::{self, Endpoint},
    profile,
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND},
        System::{Com::*, Threading::CreateMutexW},
        UI::Shell::{
            FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
            IFileOpenDialog, SIGDN_FILESYSPATH,
        },
    },
    core::{HSTRING, w},
};
use wry::{NewWindowResponse, WebContext, WebViewBuilder, WebViewBuilderExtWindows};

const URI: &str = "uac2://localhost/index.html";
const WEB_URI: &str = "http://uac2.localhost/index.html";
fn device_operation(op: &str) -> bool {
    matches!(
        op,
        "bench" | "apply" | "restore" | "force" | "hardRestart" | "generateDiagnostic"
    )
}

fn operation_error(op: &str, message: &Value, reason: &str) -> Value {
    json!({"type":"error","op":op,"id":message["id"],"deviceId":message["device"]["id"],"requestId":message["requestId"],"message":reason})
}

enum Update {
    Send(Value),
    Finish(Value),
    Exit,
    Minimize,
    ToggleMaximize,
    Drag,
    Resize(ResizeDirection),
}

fn resize_direction(value: &Value) -> Option<ResizeDirection> {
    use ResizeDirection::*;
    Some(match value.as_str()? {
        "n" => North,
        "ne" => NorthEast,
        "e" => East,
        "se" => SouthEast,
        "s" => South,
        "sw" => SouthWest,
        "w" => West,
        "nw" => NorthWest,
        _ => return None,
    })
}
struct Instance(HANDLE);
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn page() -> Vec<u8> {
    let icon = STANDARD.encode(include_bytes!("../assets/uac2-icon.svg"));
    include_str!("../ui/index.html")
        .replace("__ICON__", &format!("data:image/svg+xml;base64,{icon}"))
        .replace(
            "<!--STYLE-->",
            &format!("<style>{}</style>", include_str!("../ui/style.css")),
        )
        .replace(
            "<!--SCRIPT-->",
            &format!("<script>{}</script>", include_str!("../ui/main.js")),
        )
        .into_bytes()
}

pub fn run() -> Result<(), String> {
    let h = unsafe { CreateMutexW(None, false, w!(r"Local\UAC2-Control.Desktop.v1")) }
        .map_err(|e| e.to_string())?;
    let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let instance = Instance(h);
    if exists {
        return Err("UAC2 Control 已在本次 Windows 会话中运行。".into());
    }
    let (init, preferences_error) = match Preferences::load_for_desktop() {
        Ok(prefs) => (prefs, None),
        Err(e) => (Preferences::default(), Some(e)),
    };
    let migration_error = profile::migrate_legacy_snapshots().err();
    let el = EventLoopBuilder::<Update>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("UAC2 Control · 共享播放周期")
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_window_icon(Some(
            Icon::from_resource(101, Some(PhysicalSize::new(32, 32))).map_err(|e| e.to_string())?,
        ))
        .with_taskbar_icon(Some(
            Icon::from_resource(101, Some(PhysicalSize::new(64, 64))).map_err(|e| e.to_string())?,
        ))
        .with_inner_size(LogicalSize::new(1140.0, 806.0))
        .with_min_inner_size(LogicalSize::new(600.0, 520.0))
        .build(&el)
        .map_err(|e| e.to_string())?;
    let hwnd = window.hwnd();
    let (send, receive) = mpsc::sync_channel::<Value>(16);
    let busy = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_input = cancelled.clone();
    let busy_input = busy.clone();
    let proxy = el.create_proxy();
    let notices = proxy.clone();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let data = exe
        .parent()
        .ok_or("无法定位程序文件夹。")?
        .join("uac2-data/WebView2");
    if let Err(e) = std::fs::create_dir_all(&data) {
        window.set_visible(false);
        let reason = if init.lang == "en" {
            format!("Could not create the WebView2 cache beside the program: {e}")
        } else {
            format!("无法在程序旁创建 WebView2 缓存：{e}")
        };
        return crate::emergency::offer(&reason, init.lang == "en", init.theme == "light");
    }
    let mut context = WebContext::new(Some(data));
    let body = page();
    let web=WebViewBuilder::new_with_web_context(&mut context)
        .with_background_color(if init.theme=="light" {(243,246,249,255)} else {(22,24,30,255)})
        .with_custom_protocol("uac2".into(),move |_,req|{
            let ok=req.uri().path()=="/index.html";
            wry::http::Response::builder().status(if ok{200}else{404})
                .header("Content-Type","text/html; charset=utf-8")
                .header("Cache-Control","no-store")
                .header("Content-Security-Policy","default-src 'none'; img-src data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; form-action 'none'")
                .body(if ok{Cow::Owned(body.clone())}else{Cow::Borrowed(&b""[..])}).unwrap()
        }).with_url(URI).with_devtools(false).with_default_context_menus(false)
        // wry 0.54 replaces its defaults when custom browser arguments are supplied.
        .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required --disk-cache-size=8388608")
        .with_browser_accelerator_keys(false)
        .with_navigation_handler(|url|matches!(url.as_str(),URI|WEB_URI|"about:blank"))
        .with_new_window_req_handler(|_,_|NewWindowResponse::Deny)
        .with_ipc_handler(move |req|{
            if (req.uri()!=WEB_URI && req.uri()!=URI)||req.body().len()>65536{return}
            let Ok(msg)=serde_json::from_str::<Value>(req.body()) else{return};
            let Some(op)=msg["op"].as_str() else{return};
            let window_action = match op {
                "exit" => Some(Update::Exit),
                "windowMinimize" => Some(Update::Minimize),
                "windowMaximize" => Some(Update::ToggleMaximize),
                "windowDrag" => Some(Update::Drag),
                "windowResize" => resize_direction(&msg["edge"]).map(Update::Resize),
                _ => None,
            };
            if let Some(action)=window_action {let _=notices.send_event(action);return}
            if op=="cancelBench" {cancel_input.store(true,Ordering::Release);return}
            let long=device_operation(op);
            if long {
                if busy_input.swap(true,Ordering::AcqRel){
                    if op=="hardRestart" || op=="generateDiagnostic" {
                        let _=notices.send_event(Update::Send(operation_error(op,&msg,"另一项设备操作仍在运行。")));
                    }
                    return
                }
                cancel_input.store(false,Ordering::Release);
            }
            if send.try_send(msg.clone()).is_err(){
                let err=operation_error(op,&msg,"操作队列正忙，请重试。");
                let _=notices.send_event(if long{Update::Finish(err)}else{Update::Send(err)});
            }
        }).build(&window);
    let web = match web {
        Ok(web) => web,
        Err(e) => {
            window.set_visible(false);
            let reason = if init.lang == "en" {
                format!("Could not initialize the WebView2 window: {e}")
            } else {
                format!("无法创建 WebView2 窗口：{e}")
            };
            return crate::emergency::offer(&reason, init.lang == "en", init.theme == "light");
        }
    };
    let cancel_worker = cancelled.clone();
    thread::Builder::new()
        .name("UAC2 desktop operations".into())
        .spawn(move || {
            let post = |x| {
                let _ = proxy.send_event(x);
            };
            let mut controller = Controller {
                prefs: init,
                preferences_error,
                migration_error,
                session_consent: false,
                shown: Vec::new(),
                owner: hwnd,
                selected: String::new(),
                disconnected: None,
                rebuilt: None,
                restart: None,
            };
            loop {
                let message = match receive.recv_timeout(Duration::from_secs(2)) {
                    Ok(message) => message,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(changed) = controller.watch_format() {
                            post(Update::Send(changed));
                        }
                        continue;
                    }
                };
                let op = message["op"].as_str().unwrap_or("");
                let long = device_operation(op);
                let result = controller.handle(op, &message, &cancel_worker, &post);
                match result {
                    Ok(Some(msg)) => post(if long {
                        Update::Finish(msg)
                    } else {
                        Update::Send(msg)
                    }),
                    Ok(None) => {}
                    Err(e) => {
                        let err = operation_error(op, &message, &e);
                        post(if long {
                            Update::Finish(err)
                        } else {
                            Update::Send(err)
                        });
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let mut deferred_close = false;
    el.run(move |event, _, control| {
        let _keep = (&instance, &window, &context);
        *control = ControlFlow::Wait;
        match event {
            Event::UserEvent(Update::Minimize) => window.set_minimized(true),
            Event::UserEvent(Update::ToggleMaximize) => {
                window.set_maximized(!window.is_maximized())
            }
            Event::UserEvent(Update::Drag) => {
                let _ = window.drag_window();
            }
            Event::UserEvent(Update::Resize(edge)) if !window.is_maximized() => {
                let _ = window.drag_resize_window(edge);
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                let _ = web.evaluate_script(&format!(
                    "window.onNative?.({{type:'windowState',maximized:{}}});",
                    window.is_maximized()
                ));
            }
            Event::UserEvent(Update::Send(data)) => {
                let _ = web.evaluate_script(&format!("window.onNative?.({data});"));
            }
            Event::UserEvent(Update::Finish(data)) => {
                busy.store(false, Ordering::Release);
                if deferred_close {
                    *control = ControlFlow::Exit
                } else {
                    let _ = web.evaluate_script(&format!("window.onNative?.({data});"));
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(Update::Exit) => {
                if busy.load(Ordering::Acquire) {
                    cancelled.store(true, Ordering::Release);
                    deferred_close = true
                } else {
                    *control = ControlFlow::Exit
                }
            }
            _ => {}
        }
    })
}

struct RestartConfirmation {
    request_id: u64,
    token: String,
    endpoint: String,
    issued: Instant,
    plan: profile::HardRestart,
}

struct Controller {
    prefs: Preferences,
    preferences_error: Option<String>,
    migration_error: Option<String>,
    session_consent: bool,
    shown: Vec<Endpoint>,
    owner: isize,
    selected: String,
    disconnected: Option<String>,
    rebuilt: Option<Endpoint>,
    restart: Option<RestartConfirmation>,
}

impl Controller {
    fn watch_format(&mut self) -> Option<Value> {
        if !(self.prefs.accepted || self.session_consent) || self.rebuilt.is_some() {
            return None;
        }
        let wanted = self.disconnected.as_ref().unwrap_or(&self.selected);
        if wanted.is_empty() {
            return None;
        }
        let old = self.shown.iter().find(|d| &d.id == wanted);
        match (devices::formats(wanted), old) {
            (Ok((raw, mix)), Some(d)) if raw == d.raw_format && mix == d.mix_raw => return None,
            (Err(_), None) => return None,
            _ => {}
        }
        // The device may go offline briefly as Windows changes the mix format.
        // Remember the exact ID without redirecting the selection to another output.
        let items = devices::list().ok()?;
        let now = items.iter().find(|d| &d.id == wanted);
        if matches!((old, now), (Some(a), Some(b)) if
            a.raw_format == b.raw_format && a.mix_raw == b.mix_raw && a.supported == b.supported)
            || (old.is_none() && now.is_none())
        {
            return None;
        }
        let id = wanted.clone();
        self.restart = None;
        self.selected = now.map(|d| d.id.clone()).unwrap_or_default();
        self.disconnected = now.is_none().then_some(id);
        self.shown = items;
        Some(
            json!({"type":"devices","automatic":true,"selected":self.selected,"devices":self.shown}),
        )
    }
    fn init(&self) -> Value {
        json!({"type":"init","accepted":self.prefs.accepted || self.session_consent,"sessionConsent":self.session_consent,"lang":self.prefs.lang,
               "theme":self.prefs.theme,"hasDir":self.prefs.directory.is_some(),"path":self.prefs.directory,
               "version":env!("CARGO_PKG_VERSION"), "preferencesError":self.preferences_error, "migrationError":self.migration_error,
               "diagnosticPath":std::env::current_exe().ok().and_then(|p| p.parent().map(|dir| dir.join("uac2-data/diagnostics/latest.json")))})
    }
    fn target(
        &self,
        msg: &Value,
        matches: fn(&Endpoint, &Endpoint) -> bool,
        check_periods: bool,
    ) -> Result<Endpoint, String> {
        let echoed: Endpoint = serde_json::from_value(msg["device"].clone())
            .map_err(|_| "所选设备的显示信息无效。")?;
        let old = self
            .shown
            .iter()
            .find(|d| d.id == self.selected && d.id == echoed.id)
            .ok_or("所选播放设备已变化，请刷新。")?;
        let fresh = if check_periods {
            devices::get(&old.id)?
        } else {
            devices::get_for_rebuild(&old.id)?
        };
        if !matches(old, &echoed)
            || !matches(old, &fresh)
            || check_periods
                && (fresh.min != old.min
                    || fresh.default != old.default
                    || fresh.fundamental != old.fundamental
                    || fresh.max != old.max)
        {
            return Err("设备、驱动、格式或设置已经变化，请刷新并重新确认。".into());
        }
        Ok(fresh)
    }
    fn save(&mut self, mutate: impl FnOnce(&mut Preferences)) -> Result<(), String> {
        if let Some(error) = &self.preferences_error {
            return Err(format!(
                "未能完整读取原有偏好设置。请检查 uac2-data 后重开程序。详情：{error}"
            ));
        }
        let mut prefs = self.prefs.clone();
        mutate(&mut prefs);
        prefs.save().map_err(|e| {
            format!("无法保存同意记录或偏好设置。请检查 uac2-data 是否可写。详情：{e}")
        })?;
        self.prefs = prefs;
        Ok(())
    }
    fn handle(
        &mut self,
        op: &str,
        m: &Value,
        cancel: &AtomicBool,
        post: &impl Fn(Update),
    ) -> Result<Option<Value>, String> {
        match op {
            "ready" => return Ok(Some(self.init())),
            "accept" => {
                self.save(|p| p.accepted = true)?;
                self.session_consent = false;
                return Ok(Some(self.init()));
            }
            "acceptSession" => {
                self.session_consent = true;
                return Ok(Some(self.init()));
            }
            "prefs" => {
                let lang = m["lang"].as_str().ok_or("无效的语言")?;
                let theme = m["theme"].as_str().ok_or("无效的主题")?;
                if !matches!(lang, "zh" | "en") || !matches!(theme, "dark" | "light") {
                    return Err("无效的偏好设置。".into());
                }
                self.save(|p| {
                    p.lang = lang.into();
                    p.theme = theme.into()
                })?;
                return Ok(None);
            }
            _ => {}
        }
        if !(self.prefs.accepted || self.session_consent) {
            return Err("请先阅读并确认启动警告。".into());
        }
        if matches!(op, "apply" | "restore" | "prepareRestart" | "hardRestart")
            && let Some(error) = &self.migration_error
        {
            return Err(format!(
                "恢复快照未能完整载入。核对设备身份后可由 Windows 重建原厂设置。详情：{error}"
            ));
        }
        let reply = match op {
            "openIssue" | "openRepository" => {
                bench_worker::open_project_link(op == "openIssue")?;
                json!({"type":"linkOpened","op":op})
            }
            "generateDiagnostic" => {
                let endpoint = m["id"]
                    .as_str()
                    .filter(|id| !id.is_empty() && *id == self.selected)
                    .and_then(|id| self.shown.iter().find(|d| d.id == id))
                    .ok_or("所选播放设备已变化，请刷新。")?;
                let path = uac2_control::diagnostics::write_report(&endpoint.id)?;
                json!({"type":"diagnostic","path":path,"id":endpoint.id,"requestId":m["requestId"]})
            }
            "list" | "refresh" => {
                self.restart = None;
                let items = devices::list()?;
                let request_id = m["id"].as_str().unwrap_or(&self.selected);
                let wanted = if request_id.is_empty() && op == "refresh" {
                    self.disconnected.as_deref().unwrap_or(request_id)
                } else {
                    request_id
                }
                .to_owned();
                let rebuilt = self.rebuilt.take().filter(|e| e.id == wanted);
                self.selected = if let Some(before) = rebuilt {
                    profile::rediscover(&before, &self.shown, &items).unwrap_or_default()
                } else if wanted.is_empty() && op == "list" {
                    items.first().map(|d| d.id.clone()).unwrap_or_default()
                } else {
                    items
                        .iter()
                        .find(|d| d.id == wanted)
                        .map(|d| d.id.clone())
                        .unwrap_or_default()
                };
                self.disconnected =
                    (self.selected.is_empty() && !wanted.is_empty()).then(|| wanted.to_owned());
                self.shown = items;
                json!({"type":"devices","devices":self.shown,"selected":self.selected})
            }
            "prepareRestart" => {
                self.restart = None;
                let request_id = m["requestId"]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .ok_or("缺少硬重启确认编号。")?;
                let target = self.target(m, devices::matches_target, true)?;
                if m["id"].as_str() != Some(&target.id) {
                    return Err("硬重启目标与所选设备不符。".into());
                }
                let plan = profile::prepare_hard_restart(&target)?;
                let port = plan.port();
                let token = format!(
                    "{:032x}",
                    unsafe { CoCreateGuid() }
                        .map_err(|e| e.to_string())?
                        .to_u128()
                );
                let reply = json!({"type":"restartPrepared","op":op,"id":target.id,
                    "requestId":request_id,"token":token,"port":port});
                self.restart = Some(RestartConfirmation {
                    request_id,
                    token,
                    endpoint: target.id,
                    issued: Instant::now(),
                    plan,
                });
                reply
            }
            "cancelRestart" => {
                if self
                    .restart
                    .as_ref()
                    .is_some_and(|p| Some(p.request_id) == m["requestId"].as_u64())
                {
                    self.restart = None;
                }
                return Ok(None);
            }
            "hardRestart" => {
                let matching = self.restart.as_ref().is_some_and(|p| {
                    Some(p.request_id) == m["requestId"].as_u64()
                        && Some(p.token.as_str()) == m["token"].as_str()
                        && Some(p.endpoint.as_str()) == m["id"].as_str()
                });
                if !matching {
                    return Err("硬重启确认已失效，请重新选择。".into());
                }
                let pending = self.restart.take().ok_or("硬重启确认已失效。")?;
                if pending.issued.elapsed() >= Duration::from_secs(30) {
                    return Err("硬重启确认已过期，请重新选择。".into());
                }
                let target = self.target(m, devices::matches_target, true)?;
                if target.id != pending.endpoint {
                    return Err("所选设备已变化，请重新确认。".into());
                }
                let result = profile::hard_restart(pending.plan);
                uac2_control::diagnostics::remember_operation(&target, op, &result, false);
                json!({"type":"operation","op":op,"id":target.id,"requestId":pending.request_id,
                    "ok":result.is_ok(),"message":result.unwrap_or_else(|err|err)})
            }
            "apply" | "restore" | "force" => {
                self.restart = None;
                let target = if op == "force" {
                    self.target(m, devices::matches_rebuild_target, false)?
                } else {
                    self.target(m, devices::matches_target, true)?
                };
                let ms = m["ms"].as_f64().unwrap_or(0.0);
                if op == "apply" && (!ms.is_finite() || !(0.5..=10.0).contains(&ms)) {
                    return Err("无效的最小周期。".into());
                }
                let mut offer_restart = false;
                let result = profile::run_with_restart_offer(
                    op,
                    &target,
                    ms,
                    m["all"].as_bool().unwrap_or(false),
                    &mut offer_restart,
                );
                uac2_control::diagnostics::remember_operation(&target, op, &result, offer_restart);
                if op == "force" && result.is_ok() {
                    self.rebuilt = Some(target.clone());
                }
                json!({"type":"operation","op":op,"id":target.id,"requestId":m["requestId"],
                    "ok":result.is_ok(),"offerRestart":offer_restart,"message":result.unwrap_or_else(|e|e)})
            }
            "checkDir" => match self.prefs.directory.as_ref() {
                None => {
                    json!({"type":"directory","checked":true,"ok":false,"path":null,"message":""})
                }
                Some(path) => match benchmark::validate_dir(path) {
                    Ok(dir) => json!({"type":"directory","checked":true,"ok":true,"path":dir}),
                    Err(e) => {
                        json!({"type":"directory","checked":true,"ok":false,"path":path,"message":e})
                    }
                },
            },
            "chooseDir" => match pick_folder(self.owner, self.prefs.lang == "en")? {
                None => json!({"type":"directory","ok":false,"cancelled":true}),
                Some(path) => match benchmark::validate_dir(&path) {
                    Ok(dir) => {
                        self.save(|p| p.directory = Some(dir.clone()))?;
                        json!({"type":"directory","ok":true,"path":dir})
                    }
                    Err(e) => json!({"type":"directory","ok":false,"path":path,"message":e}),
                },
            },
            "bench" => {
                self.restart = None;
                let target = self.target(m, devices::matches_benchmark_target, true)?;
                let Some(dir) = self.prefs.directory.as_ref() else {
                    return Ok(Some(
                        json!({"type":"needDirectory","deviceId":target.id,"requestId":m["requestId"],"message":""}),
                    ));
                };
                let dir = match benchmark::validate_dir(dir) {
                    Ok(dir) => dir,
                    Err(e) => {
                        return Ok(Some(
                            json!({"type":"needDirectory","deviceId":target.id,"requestId":m["requestId"],"message":e}),
                        ));
                    }
                };
                let result = bench_worker::measure(
                    &target,
                    &dir,
                    |mut p| {
                        p["type"] = json!("progress");
                        p["requestId"] = m["requestId"].clone();
                        p["deviceId"] = json!(target.id);
                        post(Update::Send(p));
                    },
                    || cancel.load(Ordering::Acquire),
                )?;
                json!({"type":"result","deviceId":target.id,"requestId":m["requestId"],"data":result})
            }
            _ => return Err("未知操作。".into()),
        };
        Ok(Some(reply))
    }
}

fn pick_folder(owner: isize, en: bool) -> Result<Option<PathBuf>, String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe { CoUninitialize() }
            }
        }
        let _com = Com;
        let result = (|| -> windows::core::Result<Option<PathBuf>> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(
                dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST,
            )?;
            dialog.SetTitle(&HSTRING::from(if en {
                "Choose the osu! (lazer) installation folder or its current subfolder"
            } else {
                "请选择 osu!（lazer）安装文件夹，也可选择其中的 current 文件夹"
            }))?;
            if let Err(e) = dialog.Show(Some(HWND(owner as _))) {
                if e.code().0 as u32 == 0x800704c7 {
                    return Ok(None);
                }
                return Err(e);
            }
            let raw = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = raw.to_string();
            CoTaskMemFree(Some(raw.0.cast()));
            Ok(Some(PathBuf::from(path?)))
        })();
        result.map_err(|e| e.to_string())
    }
}
