//! Native recovery for an unavailable WebView, independent of browser and application storage.
mod window;

use std::{
    sync::mpsc::{self, Receiver},
    thread,
};
use uac2_control::{
    devices::{self, Endpoint},
    profile,
};
use window::{Action, Window};

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Intro,
    Finding,
    Select,
    Confirm,
    Working,
    Result,
}

enum Reply {
    Devices(Result<Vec<Endpoint>, String>),
    Finished(Result<String, String>),
}

pub(crate) fn force_summary(result: &profile::ForceResult, en: bool) -> String {
    if !en {
        return result.message.clone();
    }
    let snapshot = match result.snapshot_status {
        "saved" => {
            "The rebuilt settings were verified and saved as this output’s recovery snapshot."
        }
        "preserved" => "The existing recovery snapshot was verified and kept.",
        _ => {
            "The recovery snapshot could not be verified or saved. Review the details below. You can generate a report from Diagnostic probe in the main window."
        }
    };
    format!(
        "Windows rediscovered {}. {snapshot} Check your default output and volume before you resume playback.\n\nTechnical details as reported:\n{}",
        result.endpoint.name, result.message
    )
}

pub(crate) fn force_error(error: String, en: bool) -> String {
    if let Some(message) = profile::force_blocked_message(&error, en) {
        return message;
    }
    if let Some(message) = profile::force_reboot_message(&error, en) {
        return message;
    }
    if en {
        format!(
            "Recovery did not finish. Refresh the devices and check which outputs Windows shows before taking further action.\n{error}"
        )
    } else {
        format!("恢复未完成。请先刷新设备，核对 Windows 中可用的播放输出。\n{error}")
    }
}

struct Recovery<'a> {
    window: Window,
    reason: &'a str,
    en: bool,
    screen: Screen,
    devices: Vec<Endpoint>,
    choice: Option<usize>,
    receiver: Option<Receiver<Reply>>,
    result: String,
    failed: bool,
}

impl Recovery<'_> {
    fn text(&self, zh: &'static str, en: &'static str) -> &'static str {
        if self.en { en } else { zh }
    }

    fn render(&self) {
        let t = |zh, en| self.text(zh, en);
        let (heading, description, safe, next) = match self.screen {
            Screen::Intro => (
                t("应急恢复","Emergency recovery"),
                format!("{}\r\n\r\n{}", t("控制窗口无法打开。你仍可选择一台已核对的微软 USB Audio 2.0 声卡，让 Windows 重建它的驱动默认设置。", "The control window could not open. You can still select a verified Microsoft USB Audio 2.0 device and have Windows rebuild its driver defaults."), self.reason),
                t("退出","Exit"), t("选择设备","Select device")),
            Screen::Finding => (t("正在查找声卡","Finding sound devices"), t("正在读取 Windows 的活动播放设备。","Reading the active playback devices reported by Windows.").to_owned(), t("取消","Cancel"), ""),
            Screen::Select => (t("选择要恢复的声卡","Select a device to restore"), t("选择已核对的播放输出，然后查看恢复操作的影响。","Choose a verified playback output to review the recovery action.").to_owned(),t("取消","Cancel"),t("查看恢复确认","Review recovery")),
            Screen::Confirm => {
                let d = &self.devices[self.choice.expect("confirmation requires an explicit selection")];
                let risk = t("请降低音量，暂停这块声卡的播放和录音。确认后会先检查同卡占用；若仍有占用，请退出使用它的应用，再重新确认。Windows 将卸载此声卡的 USB 音频功能，保留微软驱动包并重建默认设置。原有快照会保留，若所选输出缺少快照，将在重建并核实设置后补存。同卡音频会中断，可能出现爆音、音量重置及默认输出或端点 ID 变化。声卡未返回时，程序最多尝试一次已核对的 USB 端口重连；必要时需手动重插或重启电脑。", "Lower the volume and pause playback and recording on this card. After you confirm, the tool checks whether the card is in use. If it is still busy, quit the apps using it and confirm again. Windows will remove the USB audio function, keep the Microsoft driver package and rebuild the defaults. Existing snapshots are kept. If this output has no snapshot, its rebuilt settings are saved once verified. Audio on the card will stop. Pops, a volume reset, or changes to the default output or endpoint ID are possible. If the card does not return, the tool may reconnect its verified USB port once. You may need to reconnect it manually or restart Windows.");
                (t("确认强制恢复","Confirm force restore"),format!("{}\r\n{}\r\n{}\r\n\r\n{}",d.name,d.function_id,d.id,risk),t("返回","Back"),t("确认强制恢复","Confirm force restore"))
            }
            Screen::Working => (t("正在恢复设备","Restoring the device"), t("正在等待 Windows 重新识别声卡并核对结果。完成后即可关闭窗口。","Waiting for Windows to rediscover the card and verify the result. You can close this window when the operation finishes.").to_owned(), "", ""),
            Screen::Result => (if self.failed {t("恢复未完成","Recovery incomplete")} else {t("恢复结果","Recovery result")},self.result.clone(),t("关闭","Close"),if self.failed {t("刷新设备","Refresh devices")} else {""}),
        };
        self.window.show_page(
            heading,
            &description,
            safe,
            next,
            self.screen == Screen::Select,
            self.screen == Screen::Confirm,
            self.screen == Screen::Working,
            self.screen != Screen::Select || self.choice.is_some(),
        );
    }

    fn finish(&mut self, result: Result<String, String>) {
        self.failed = result.is_err();
        self.result = result.unwrap_or_else(|err| err);
        self.screen = Screen::Result;
        self.render();
    }

    fn run_worker(&mut self, task: impl FnOnce() -> Reply + Send + 'static) {
        let (sender, receiver) = mpsc::channel();
        match thread::Builder::new()
            .name("UAC2 native recovery".into())
            .spawn(move || {
                let _ = sender.send(task());
            }) {
            Ok(_) => self.receiver = Some(receiver),
            Err(e) => self.finish(Err(e.to_string())),
        }
    }

    fn advance(&mut self) {
        match self.screen {
            Screen::Intro | Screen::Result if self.screen == Screen::Intro || self.failed => {
                self.choice = None;
                self.screen = Screen::Finding;
                self.render();
                self.run_worker(|| Reply::Devices(devices::list_for_rebuild()));
            }
            Screen::Select => {
                if self.choice.is_some() {
                    self.screen = Screen::Confirm;
                    self.render();
                }
            }
            Screen::Confirm => {
                let Some(d) = self.choice.and_then(|i| self.devices.get(i)).cloned() else {
                    return;
                };
                let changed = self.text(
                    "设备身份已变化，请重新选择。",
                    "The device identity changed. Select the device again.",
                );
                self.screen = Screen::Working;
                self.render();
                let en = self.en;
                self.run_worker(move || {
                    let result = (|| {
                        let fresh = devices::get_for_rebuild(&d.id)?;
                        if !devices::matches_rebuild_target(&d, &fresh) {
                            return Err(changed.to_owned());
                        }
                        profile::run_force(&fresh).map(|result| force_summary(&result, en))
                    })();
                    Reply::Finished(result.map_err(|error| force_error(error, en)))
                });
            }
            _ => {}
        }
    }

    fn poll(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        let reply = match receiver.try_recv() {
            Ok(reply) => reply,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Reply::Finished(Err(self
                .text(
                    "后台任务意外结束。",
                    "The background operation ended unexpectedly.",
                )
                .to_owned())),
        };
        self.receiver = None;
        match reply {
            Reply::Finished(result) => self.finish(result),
            Reply::Devices(Err(e)) => self.finish(Err(e)),
            Reply::Devices(Ok(devices)) => {
                self.devices = devices.into_iter().filter(|d| d.can_rebuild).collect();
                if self.devices.is_empty() {
                    self.finish(Err(self
                        .text(
                            "没有找到身份可核实的活动微软 UAC2 播放设备。",
                            "No verified active Microsoft UAC2 playback device was found.",
                        )
                        .to_owned()));
                    return;
                }
                self.window.set_choices(
                    self.devices
                        .iter()
                        .map(|d| format!("{}  ·  {}", d.name, d.function_id)),
                );
                self.screen = Screen::Select;
                self.render();
            }
        }
    }
}

pub(crate) fn offer(reason: &str, en: bool, light: bool) -> Result<(), String> {
    let window = Window::open(en, light).map_err(|e| e.to_string())?;
    let mut app = Recovery {
        window,
        en,
        reason,
        screen: Screen::Intro,
        devices: Vec::new(),
        choice: None,
        receiver: None,
        result: String::new(),
        failed: false,
    };
    app.render();
    while let Some(action) = app.window.next_action().map_err(|e| e.to_string())? {
        match action {
            Action::Next if app.screen != Screen::Confirm => app.advance(),
            Action::Confirm if app.screen == Screen::Confirm => app.advance(),
            Action::Select(index) if app.screen == Screen::Select => {
                app.choice = index.filter(|i| *i < app.devices.len());
                app.window.enable_next(app.choice.is_some());
            }
            Action::Back if app.screen == Screen::Confirm => {
                app.screen = Screen::Select;
                app.render();
            }
            Action::Back | Action::Close if app.screen != Screen::Working => break,
            Action::Tick => app.poll(),
            _ => {}
        }
    }
    Ok(())
}
