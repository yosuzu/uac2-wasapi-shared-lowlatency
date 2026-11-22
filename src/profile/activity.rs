use super::{Result, native};
use crate::devices::{self, Endpoint};
use windows::{
    Win32::{
        Foundation::E_POINTER,
        Media::Audio::*,
        System::Com::{CLSCTX_ALL, CoTaskMemFree},
    },
    core::{HSTRING, Interface},
};

const PLAYBACK: (&str, &str) = (
    "强制恢复尚未卸载设备。这块声卡的播放输出正被占用。请停止播放，再重新确认。如果仍提示占用，请退出使用这块声卡的程序。",
    "The device has not been removed. A playback output on this card is in use. Stop playback, then confirm force restore again. If the output is still busy, close the app using it.",
);
const RECORDING: (&str, &str) = (
    "强制恢复尚未卸载设备。这块声卡的录音输入正被占用。请停止录音，再重新确认。如果仍提示占用，请退出使用这块声卡的程序。",
    "The device has not been removed. A recording input on this card is in use. Stop recording, then confirm force restore again. If the input is still busy, close the app using it.",
);
const BOTH: (&str, &str) = (
    "强制恢复尚未卸载设备。这块声卡的播放输出和录音输入都在使用。请停止播放和录音，再重新确认。如果仍提示占用，请退出使用这块声卡的程序。",
    "The device has not been removed. Playback and recording on this card are both in use. Stop both, then confirm force restore again. If the card is still busy, close the apps using it.",
);
const UNKNOWN: (&str, &str) = (
    "强制恢复尚未卸载设备。无法核对这块声卡的播放和录音状态。请刷新后重新确认。",
    "The device has not been removed. Playback and recording status could not be verified for this card. Refresh the device list and confirm again.",
);

pub fn force_blocked_message(error: &str, en: bool) -> Option<String> {
    for (zh, english) in [PLAYBACK, RECORDING, BOTH] {
        if error == zh {
            return Some(if en { english } else { zh }.to_owned());
        }
    }
    error
        .strip_prefix(UNKNOWN.0)
        .map(|detail| format!("{}{detail}", if en { UNKNOWN.1 } else { UNKNOWN.0 }))
}

fn has_running_session(ep: &IMMDevice) -> Result<bool> {
    unsafe {
        let manager: IAudioSessionManager2 = ep.Activate(CLSCTX_ALL, None).map_err(native::err)?;
        let sessions = manager.GetSessionEnumerator().map_err(native::err)?;
        let count = sessions.GetCount().map_err(native::err)?;
        if count < 0 {
            return Err("Windows 返回了无效的音频会话数量。".into());
        }
        for index in 0..count {
            let control = sessions.GetSession(index).map_err(native::err)?;
            if session_running(control.GetState().map_err(native::err)?)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn session_running(state: AudioSessionState) -> Result<bool> {
    if state == AudioSessionStateActive {
        Ok(true)
    } else if state == AudioSessionStateInactive || state == AudioSessionStateExpired {
        Ok(false)
    } else {
        Err("Windows 返回了未知的音频会话状态。".into())
    }
}

/// Shared sessions do not reveal exclusive ownership. Initialize without Start
/// so Windows checks ownership without playing samples or capturing audio.
fn reserve_shared(ep: &IMMDevice) -> windows::core::Result<IAudioClient> {
    unsafe {
        let client: IAudioClient = ep.Activate(CLSCTX_ALL, None)?;
        let format = client.GetMixFormat()?;
        if format.is_null() {
            return Err(E_POINTER.into());
        }
        let initialized = client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_NOPERSIST,
            0,
            0,
            format,
            None,
        );
        CoTaskMemFree(Some(format.cast()));
        initialized?;
        Ok(client)
    }
}

/// None means Windows identified an owner. A different initialization error
/// can only be classified as busy if a running shared session confirms that.
fn idle_client<T>(
    reservation: windows::core::Result<T>,
    running: impl FnOnce() -> Result<bool>,
) -> Result<Option<T>> {
    match reservation {
        Ok(client) => Ok(Some(client)),
        Err(err) if err.code() == AUDCLNT_E_DEVICE_IN_USE => Ok(None),
        Err(err) => match running() {
            Ok(true) => Ok(None),
            Ok(false) => Err(native::err(err)),
            Err(detail) => Err(format!("{} · {detail}", native::err(err))),
        },
    }
}

#[derive(Default)]
struct Activity {
    playing: bool,
    recording: bool,
    unreadable: Option<String>,
}

impl Activity {
    fn observe(&mut self, direction: EDataFlow, state: Result<bool>) {
        match state {
            Ok(true) if direction == eRender => self.playing = true,
            Ok(true) if direction == eCapture => self.recording = true,
            Ok(false) => (),
            Ok(true) => self.unknown("Windows 返回了未知的端点方向。".into()),
            Err(reason) => self.unknown(reason),
        }
    }

    fn unknown(&mut self, reason: String) {
        self.unreadable.get_or_insert(reason);
    }

    fn require_idle(self) -> Result<()> {
        match (self.playing, self.recording) {
            (true, true) => Err(BOTH.0.into()),
            (true, false) => Err(PLAYBACK.0.into()),
            (false, true) => Err(RECORDING.0.into()),
            (false, false) => match self.unreadable {
                Some(reason) => Err(format!("{} {reason}", UNKNOWN.0)),
                None => Ok(()),
            },
        }
    }
}

fn card_activity(e: &Endpoint) -> Result<Activity> {
    let enumerator = native::enumerator()?;
    let ids = devices::card_audio_endpoints(&e.physical_id)?;
    let mut result = Activity::default();
    let mut selected_active = false;
    let mut reserved = Vec::new();
    for id in ids {
        let check = (|| -> Result<()> {
            let ep = unsafe { enumerator.GetDevice(&HSTRING::from(&id)) }.map_err(native::err)?;
            match unsafe { ep.GetState() }.map_err(native::err)? {
                DEVICE_STATE_ACTIVE => (),
                DEVICE_STATE_DISABLED | DEVICE_STATE_NOTPRESENT | DEVICE_STATE_UNPLUGGED => {
                    return Ok(());
                }
                _ => return Err("Windows 返回了未知的端点状态。".into()),
            }
            let endpoint: IMMEndpoint = ep.cast().map_err(native::err)?;
            let flow = unsafe { endpoint.GetDataFlow() }.map_err(native::err)?;
            if flow != eRender && flow != eCapture {
                return Err("Windows 返回了未知的端点方向。".into());
            }
            if flow == eRender && id.eq_ignore_ascii_case(&e.id) {
                selected_active = true;
            }
            match idle_client(reserve_shared(&ep), || has_running_session(&ep))? {
                Some(client) => reserved.push((ep, flow, client)),
                None => result.observe(flow, Ok(true)),
            }
            Ok(())
        })();
        if let Err(reason) = check {
            result.unknown(format!("{id} · {reason}"));
        }
    }
    if !selected_active {
        result.unknown("未找到已确认的活动播放输出。".into());
    }
    // Keep all idle clients alive through the final session sweep. Drop them
    // before PnP removal so the tool's own handles cannot hold the card open.
    for (ep, direction, _client) in &reserved {
        result.observe(*direction, has_running_session(ep));
    }
    Ok(result)
}

pub(super) fn require_idle(e: &Endpoint) -> Result<()> {
    card_activity(e)
        .map_err(|reason| format!("{} {reason}", UNKNOWN.0))?
        .require_idle()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exclusive_owner_blocks_even_when_shared_sessions_are_empty() {
        let busy = idle_client::<()>(Err(AUDCLNT_E_DEVICE_IN_USE.into()), || Ok(false)).unwrap();
        assert!(busy.is_none());
        assert!(idle_client::<()>(Err(E_POINTER.into()), || Ok(false)).is_err());
        assert!(
            idle_client::<()>(Err(E_POINTER.into()), || Ok(true))
                .unwrap()
                .is_none()
        );
        assert!(session_running(AudioSessionStateActive).unwrap());
        assert!(!session_running(AudioSessionStateInactive).unwrap());
        assert!(!session_running(AudioSessionStateExpired).unwrap());
        assert!(session_running(AudioSessionState(10)).is_err());
    }

    #[test]
    fn force_requires_all_checked_directions_to_be_idle() {
        let mut inactive = Activity::default();
        inactive.observe(eRender, Ok(false));
        inactive.observe(eCapture, Ok(false));
        assert!(inactive.require_idle().is_ok());
        for (states, expected) in [
            (vec![(eRender, Ok(true)), (eRender, Ok(false))], PLAYBACK),
            (vec![(eRender, Ok(false)), (eCapture, Ok(true))], RECORDING),
            (vec![(eRender, Ok(true)), (eCapture, Ok(true))], BOTH),
        ] {
            let mut card = Activity::default();
            for (direction, state) in states {
                card.observe(direction, state);
            }
            let blocked = card.require_idle().unwrap_err();
            assert_eq!(blocked, expected.0);
            assert_eq!(force_blocked_message(&blocked, true).unwrap(), expected.1);
        }
        let mut unreadable = Activity::default();
        unreadable.observe(eCapture, Err("Windows 0x1234".into()));
        let blocked = unreadable.require_idle().unwrap_err();
        assert_eq!(
            force_blocked_message(&blocked, true).unwrap(),
            format!("{} Windows 0x1234", UNKNOWN.1)
        );
        assert!(force_blocked_message("卸载返回 5", true).is_none());
    }
}
