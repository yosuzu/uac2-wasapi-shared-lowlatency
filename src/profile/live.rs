//! Explicitly invoked hardware acceptance. The test emits evidence to stdout.
//! No path supplied by the caller is opened by the elevated process.
use super::*;
use serde_json::{Value, json};
use std::{env, fmt::Write, io::Write as _};

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").unwrap();
    }
    s
}
fn record(value: Value) {
    // The administrator's Windows console can use a legacy OEM code page.
    // ASCII JSON escapes keep the saved receipt lossless across that boundary.
    let mut line = String::new();
    for c in value.to_string().chars() {
        if c.is_ascii() {
            line.push(c)
        } else {
            for u in c.encode_utf16(&mut [0; 2]) {
                write!(&mut line, "\\u{u:04x}").unwrap()
            }
        }
    }
    // A closed evidence pipe must not interrupt a hardware test's recovery.
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "UAC2-EVIDENCE {line}");
    let _ = out.flush();
}
fn capture(e: &Endpoint) -> Value {
    let result = native::read(e);
    let (b, p) = result
        .as_ref()
        .map(|(b, p)| (Some(hex(b)), Some(hex(p))))
        .unwrap_or((None, None));
    json!({"endpoint":e,"payload":b,"registry":p,"readError":result.err(),"settings":native::settings(e).ok()})
}

#[test]
#[ignore = "Removes only the completed acceptance snapshot matching the pinned pre-test evidence"]
fn live_cleanup() {
    let _com = native::Apartment::enter().unwrap();
    let e = devices::get(&env::var("UAC2_LIVE_ID").unwrap()).unwrap();
    assert!(
        e.supported
            && e.physical_id
                .eq_ignore_ascii_case(&env::var("UAC2_LIVE_PHYSICAL").unwrap())
            && e.function_id
                .eq_ignore_ascii_case(&env::var("UAC2_LIVE_FUNCTION").unwrap())
            && e.container
                .eq_ignore_ascii_case(&env::var("UAC2_LIVE_CONTAINER").unwrap())
    );
    let expected = capability::hex(&env::var("UAC2_LIVE_TEST_BASELINE").unwrap()).unwrap();
    assert_eq!(native::read(&e).unwrap().0, expected);
    let removed = store::remove_verified_test_snapshot(&e, &expected).unwrap();
    record(json!({"event":"test-cleanup","removed":removed,"snapshot":has_snapshot(&e)}));
}

#[test]
#[ignore = "Temporarily lowers one explicitly named output for a separate non-administrator benchmark"]
fn live_benchmark_window() {
    let _com = native::Apartment::enter().unwrap();
    let e = devices::get(&env::var("UAC2_LIVE_ID").unwrap()).unwrap();
    assert!(e.supported && unsafe { windows::Win32::UI::Shell::IsUserAnAdmin() }.as_bool());
    for (key, value) in [
        ("UAC2_LIVE_PHYSICAL", &e.physical_id),
        ("UAC2_LIVE_FUNCTION", &e.function_id),
        ("UAC2_LIVE_CONTAINER", &e.container),
    ] {
        assert!(value.eq_ignore_ascii_case(&env::var(key).unwrap()));
    }
    let baseline = native::read(&e).unwrap().0;
    let original_snapshot = store::load(&e).unwrap();
    if let Some(s) = &original_snapshot {
        assert_eq!(
            s.original, baseline,
            "Existing customized setting must be preserved"
        )
    }
    let peers: Vec<_> = devices::list()
        .unwrap()
        .into_iter()
        .filter(|v| v.supported && same_function(&e, v))
        .map(|v| {
            let level = native::settings(&v).unwrap().level().unwrap();
            (v, level)
        })
        .collect();
    record(json!({"event":"window-before","state":capture(&e)}));
    let measurements = (|| -> Result<()> {
        let msg = run("apply", &fresh(&e)?, 3.0, false)?;
        let v = devices::get(&e.id)?;
        if v.min != (v.rate * 3).div_ceil(1000).div_ceil(v.fundamental) * v.fundamental
            || v.default != e.default
        {
            return Err("3 ms 尚未公布。".into());
        }
        for (peer, level) in &peers {
            let now = devices::get(&peer.id)?;
            if !same_function(&e, &now) {
                return Err("测试窗口中同卡端点发生变化。".into());
            }
            native::check_test_volume(&now, *level)?;
        }
        record(json!({"event":"window-ready","state":capture(&v),"message":msg}));
        let (send, recv) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            let _ = send.send(line);
        });
        match recv.recv_timeout(Duration::from_secs(240)) {
            Ok(s) if s.trim() == "restore" => Ok(()),
            _ => Err("测试进程已断开或等待超时，执行还原。".into()),
        }
    })();
    let restore = (|| -> Result<()> {
        let v = devices::get(&e.id)?;
        if native::read(&v)?.0 != baseline || range(&v) != range(&e) {
            for (peer, original_level) in &peers {
                native::check_test_volume(&devices::get(&peer.id)?, *original_level)?;
            }
            let msg = run("restore", &v, 0.0, false)?;
            record(json!({"event":"window-restore","message":msg}));
        }
        let v = devices::get(&e.id)?;
        if native::read(&v)?.0 != baseline || range(&v) != range(&e) {
            return Err("原配置复核失败。".into());
        }
        Ok(())
    })();
    let levels: Vec<_> = peers
        .iter()
        .map(|(v, (level, mute))| {
            devices::get(&v.id).and_then(|v| native::check_test_volume(&v, (*level, *mute)))
        })
        .collect();
    let cleanup = (|| -> Result<Vec<String>> {
        if restore.is_err() || levels.iter().any(|r| r.is_err()) {
            return Err("等待配置和音量复核，保留快照。".into());
        }
        let mut removed = Vec::new();
        if original_snapshot.is_none() && store::remove_verified_test_snapshot(&e, &baseline)? {
            removed.push(e.id.clone())
        }
        // The administrator's test coordinator independently binds the old
        // ID to the signed-off pre-uninstall baseline and its absent snapshot.
        if let (Ok(old_id), Ok(old_hex)) = (
            env::var("UAC2_LIVE_TEST_OLD_ID"),
            env::var("UAC2_LIVE_TEST_BASELINE"),
        ) {
            let old_bytes = capability::hex(&old_hex)?;
            if old_bytes != baseline || old_id.eq_ignore_ascii_case(&e.id) {
                return Err("重建前后的身份或原始字节不符合本次验收基线。".into());
            }
            let old = Endpoint {
                id: old_id,
                ..e.clone()
            };
            if store::remove_verified_test_snapshot(&old, &old_bytes)? {
                removed.push(old.id)
            }
        }
        Ok(removed)
    })();
    let end = devices::get(&e.id).ok().map(|v| capture(&v));
    record(
        json!({"event":"window-finished","measurement":measurements,"restore":restore,"levels":levels,"testSnapshotsRemoved":cleanup,"state":end}),
    );
    assert!(measurements.is_ok() && restore.is_ok() && levels.iter().all(|r| r.is_ok()));
}
#[test]
#[ignore = "Reads the attached Windows audio devices for an explicit acceptance session"]
fn live_baseline() {
    let _com = native::Apartment::enter().unwrap();
    let endpoints = devices::list().unwrap();
    let supported = endpoints
        .iter()
        .filter(|e| e.supported)
        .map(capture)
        .collect::<Vec<_>>();
    record(
        json!({"event":"baseline","administrator":unsafe{windows::Win32::UI::Shell::IsUserAnAdmin()}.as_bool(),
        "all":endpoints,"supported":supported}),
    );
}

#[test]
#[ignore = "One separately confirmed USB port restart on an explicitly pinned output"]
fn live_hard_restart() -> Result<()> {
    let _com = native::Apartment::enter()?;
    let _lock = native::Operation::lock()?;
    let target = devices::get(&env::var("UAC2_LIVE_ID").map_err(|e| e.to_string())?)?;
    if !unsafe { windows::Win32::UI::Shell::IsUserAnAdmin() }.as_bool() {
        return Err("需要管理员权限。".into());
    }
    for (key, value) in [
        ("UAC2_LIVE_PHYSICAL", &target.physical_id),
        ("UAC2_LIVE_FUNCTION", &target.function_id),
        ("UAC2_LIVE_CONTAINER", &target.container),
        ("UAC2_LIVE_ARRIVAL", &target.arrival.to_string()),
    ] {
        if !value.eq_ignore_ascii_case(&env::var(key).map_err(|e| e.to_string())?) {
            return Err(format!("{key} 与所选设备不符。"));
        }
    }
    let target = fresh(&target)?;
    let (original, registry) = native::read(&target)?;
    let raw = check_format(&target)?;
    let candidate =
        capability::plan(&original, &raw, target.rate, target.channels, 3.0, false)?.bytes;
    let initial = store::load(&target)?;
    let snapshot_before = serde_json::to_value(&initial).map_err(|e| e.to_string())?;
    if candidate == original
        || capability::active(&original, &raw)? != range(&target)
        || initial.as_ref().is_some_and(|s| {
            s.reload_pending
                || s.original != original
                || s.before != original
                || s.intended != original
        })
        || prepare_hard_restart(&target).is_ok()
    {
        return Err("验收需要可恢复的基线、不同的 3 ms 目标，且没有先前待重载操作。".into());
    }

    let check = |e: &Endpoint, bytes: &[u8]| -> Result<()> {
        let (now, _) = native::read(e)?;
        if now != bytes
            || range(e) != capability::active(bytes, &raw)?
            || e.raw_format != target.raw_format
            || e.mix_raw != target.mix_raw
        {
            return Err("能力正文、活动格式或实际公布的周期与预期不符。".into());
        }
        Ok(())
    };
    let peers = devices::list()?
        .into_iter()
        .filter(|v| v.supported && same_function(v, &target))
        .map(|v| {
            let settings = native::settings(&v)?;
            settings.level()?;
            let payload = native::read(&v)?.0;
            Ok((v, settings, payload))
        })
        .collect::<Result<Vec<_>>>()?;
    let verify_card = || -> Result<()> {
        for (old, settings, payload) in &peers {
            let v = devices::get(&old.id)?;
            if !v.supported
                || !same_function(old, &v)
                || v.driver_version != old.driver_version
                || v.association != old.association
                || v.form_factor != old.form_factor
                || native::settings(&v)? != *settings
            {
                return Err(format!("{} 身份、音量、静音或默认角色发生变化。", old.name));
            }
            if !old.id.eq_ignore_ascii_case(&target.id)
                && (native::read(&v)?.0 != *payload
                    || range(&v) != range(old)
                    || v.raw_format != old.raw_format
                    || v.mix_raw != old.mix_raw)
            {
                return Err(format!("{} 同卡对照能力、格式或周期发生变化。", old.name));
            }
        }
        Ok(())
    };
    record(json!({"event":"hard-before","state":capture(&target),"snapshot":snapshot_before}));
    let trial = (|| -> Result<()> {
        let applied = run("apply", &target, 3.0, false);
        let current = devices::get(&target.id)?;
        record(json!({"event":"hard-normal-apply","result":applied,"state":capture(&current)}));
        verify_card()?;
        let desired = if applied.is_ok() {
            check(&current, &candidate)?;
            // Exercise the real restore writer, including journal flush and readback.
            // Substitute only the normal restart command at its point of invocation.
            let mut reached_restart = false;
            let mut offer = false;
            let deferred = configure("restore", &fresh(&current)?, 0.0, false, &mut offer, |_| {
                reached_restart = true;
                Err("验收在常规重载命令前暂停。".into())
            });
            if !reached_restart || !offer || deferred.is_ok() {
                return Err(format!("未抵达已写入并读回后的重载点：{deferred:?}"));
            }
            &original
        } else {
            &candidate
        };
        let pending = devices::get(&target.id)?;
        let journal = store::load(&pending)?.ok_or("缺少待重载快照。")?;
        if !pending.restart_pending
            || !journal.reload_pending
            || journal.original != original
            || journal.intended != *desired
            || native::read(&pending)?.0 != *desired
        {
            return Err("当前待重载操作、受保护的原件或已提交的目标不匹配。".into());
        }
        let arrival = native::arrival(&pending.physical_id)?;
        let confirmation = prepare_hard_restart(&pending)?;
        record(
            json!({"event":"hard-confirm","hub":confirmation.hub(),"port":confirmation.port(),
            "physicalArrival":arrival.to_string(),"expectedMin":capability::active(desired,&raw)?.min,
            "state":capture(&pending)}),
        );
        let restarted = hard_restart(confirmation)?;
        let now = devices::get(&target.id)?;
        let returned = native::arrival(&now.physical_id)?;
        record(
            json!({"event":"hard-returned","message":restarted,"physicalArrival":returned.to_string(),
            "state":capture(&now)}),
        );
        if !same_function(&target, &now)
            || (returned == arrival && now.arrival == pending.arrival)
            || now.restart_pending
            || store::load(&now)?.is_none_or(|s| s.reload_pending)
            || prepare_hard_restart(&now).is_ok()
        {
            return Err("重新识别或清除本次待重载的独立核对失败。".into());
        }
        check(&now, desired)?;
        verify_card()
    })();
    let recovery = (|| -> Result<()> {
        // An error can arrive while Windows is still publishing the returned output.
        let deadline = Instant::now() + Duration::from_secs(16);
        let now = loop {
            match devices::get(&target.id) {
                Ok(v) if v.supported && same_function(&target, &v) => break v,
                _ if Instant::now() < deadline => thread::sleep(Duration::from_millis(350)),
                _ => return Err("目标尚未重新上线，保留原件以便继续普通恢复。".into()),
            }
        };
        if now.driver_version != target.driver_version
            || now.raw_format != target.raw_format
            || now.mix_raw != target.mix_raw
            || now.form_factor != target.form_factor
            || !now.association.eq_ignore_ascii_case(&target.association)
        {
            return Err("端点身份、驱动或格式发生变化，保留原件。".into());
        }
        if let Some(s) = store::load(&now)? {
            if s.original != original {
                return Err("快照原件已变化。".into());
            }
            if s.reload_pending
                || native::read(&now)?.0 != original
                || range(&now) != range(&target)
            {
                let result = run("restore", &now, 0.0, false);
                record(json!({"event":"hard-restore","result":result}));
                result?;
            }
        }
        let now = devices::get(&target.id)?;
        check(&now, &original)?;
        verify_card()?;
        if native::read(&now)?.1 != registry || now.restart_pending {
            return Err("原始持久化记录或待重载标志与基线不符。".into());
        }
        if initial.is_none() {
            store::remove_verified_test_snapshot(&now, &original)?;
        }
        if serde_json::to_value(store::load(&now)?).map_err(|e| e.to_string())? != snapshot_before {
            return Err("受保护的快照未回到原状态。".into());
        }
        Ok(())
    })();
    record(
        json!({"event":"hard-finished","trial":trial,"recovery":recovery,
        "state":devices::get(&target.id).ok().as_ref().map(capture)}),
    );
    recovery?;
    trial
}

/// Queries each PCM connector format at the active Windows configuration. The
/// audio client is never initialized, so this test cannot open a shared stream.
#[test]
#[ignore = "Read-only per-format WASAPI compatibility and engine-period inventory"]
fn live_format_queries() {
    use windows::{
        Win32::{Media::Audio::*, System::Com::*},
        core::HSTRING,
    };
    let _com = native::Apartment::enter().unwrap();
    let en: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.unwrap();
    for e in devices::list().unwrap().into_iter().filter(|v| v.supported) {
        let ep = unsafe { en.GetDevice(&HSTRING::from(&e.id)) }.unwrap();
        let client: IAudioClient3 = unsafe { ep.Activate(CLSCTX_ALL, None) }.unwrap();
        let props = AudioClientProperties {
            cbSize: size_of::<AudioClientProperties>() as u32,
            eCategory: AudioCategory_Other,
            Options: AUDCLNT_STREAMOPTIONS_NONE,
            ..Default::default()
        };
        unsafe { client.SetClientProperties(&props) }.unwrap();
        let (b, _) = native::read(&e).unwrap();
        let mut formats = Vec::new();
        for (index, r) in capability::parse(&b).unwrap().into_iter().enumerate() {
            let mut aligned = vec![0u32; r.raw.len().div_ceil(4)];
            unsafe {
                std::ptr::copy_nonoverlapping(
                    r.raw.as_ptr(),
                    aligned.as_mut_ptr().cast(),
                    r.raw.len(),
                )
            };
            let w = aligned.as_ptr().cast();
            let mut closest = std::ptr::null_mut();
            let support = unsafe {
                client.IsFormatSupported(AUDCLNT_SHAREMODE_SHARED, w, Some(&mut closest))
            };
            let nearest = if closest.is_null() {
                None
            } else {
                unsafe {
                    let n = size_of::<WAVEFORMATEX>() + (*closest).cbSize as usize;
                    let bytes = std::slice::from_raw_parts(closest.cast::<u8>(), n).to_vec();
                    CoTaskMemFree(Some(closest.cast()));
                    Some(hex(&bytes))
                }
            };
            let (mut default, mut step, mut min, mut max) = (0, 0, 0, 0);
            let period = unsafe {
                client.GetSharedModeEnginePeriod(w, &mut default, &mut step, &mut min, &mut max)
            };
            formats.push(json!({"index":index+1,"raw":hex(r.raw),"rate":r.rate,"channels":r.channels,
                "bits":u16::from_le_bytes(r.raw[14..16].try_into().unwrap()),"validBits":if r.raw.len()>=40{Some(u16::from_le_bytes(r.raw[18..20].try_into().unwrap()))}else{None},
                "isWindowsDeviceFormat":e.raw_format.eq_ignore_ascii_case(&hex(r.raw)),
                "isWindowsMixFormat":e.mix_raw.eq_ignore_ascii_case(&hex(r.raw)),
                "isFormatSupportedHRESULT":format!("0x{:08X}",support.0 as u32),"closest":nearest,
                "periodHRESULT":format!("0x{:08X}",period.as_ref().err().map_or(0,|v|v.code().0 as u32)),
                "period":period.is_ok().then(||json!({"default":default,"step":step,"min":min,"max":max}))}));
        }
        record(
            json!({"event":"format-queries","endpoint":e,"formats":formats,"streamOpened":false}),
        );
    }
}

// The caller explicitly pins live PnP and endpoint identities in addition to
// selecting a single action. This is never run by an ordinary cargo test.
#[test]
#[ignore = "Writes or reenumerates one explicitly identified Microsoft UAC2 device"]
fn live_action() {
    let _com = native::Apartment::enter().unwrap();
    let action = env::var("UAC2_LIVE_ACTION").unwrap();
    let id = env::var("UAC2_LIVE_ID").unwrap();
    let e = devices::get(&id).unwrap();
    assert!(e.supported && !e.physical_id.is_empty());
    for (field, actual) in [
        ("UAC2_LIVE_PHYSICAL", &e.physical_id),
        ("UAC2_LIVE_FUNCTION", &e.function_id),
        ("UAC2_LIVE_CONTAINER", &e.container),
    ] {
        assert!(actual.eq_ignore_ascii_case(&env::var(field).unwrap()));
    }
    let (command, all) = match action.as_str() {
        "current" => ("apply", false),
        "all" => ("apply", true),
        "restore" => ("restore", false),
        "force" => ("force", false),
        _ => panic!("unknown action"),
    };
    assert!(
        unsafe { windows::Win32::UI::Shell::IsUserAnAdmin() }.as_bool(),
        "hardware actions require administrator token"
    );
    record(json!({"event":"before","action":action,"target":capture(&e)}));
    let result = run(command, &e, 3.0, all);
    let after = devices::list().unwrap_or_default();
    record(
        json!({"event":"result","action":action,"ok":result.is_ok(),"message":match &result{Ok(s)|Err(s)=>s},
        "card":after.iter().filter(|v|same_function(&e,v)).map(capture).collect::<Vec<_>>()}),
    );
    result.unwrap();
}

#[test]
#[ignore = "Full live matrix, including a single uninstall of the explicitly named audio function"]
fn live_card_matrix() {
    let _com = native::Apartment::enter().unwrap();
    let id = env::var("UAC2_LIVE_ID").unwrap();
    let reference = devices::get(&id).unwrap();
    assert!(reference.supported);
    for (key, value) in [
        ("UAC2_LIVE_PHYSICAL", &reference.physical_id),
        ("UAC2_LIVE_FUNCTION", &reference.function_id),
        ("UAC2_LIVE_CONTAINER", &reference.container),
    ] {
        assert!(value.eq_ignore_ascii_case(&env::var(key).unwrap()));
    }
    assert!(
        unsafe { windows::Win32::UI::Shell::IsUserAnAdmin() }.as_bool(),
        "administrator token required"
    );
    let originals: Vec<_> = devices::list()
        .unwrap()
        .into_iter()
        .filter(|e| e.supported && same_function(&reference, e))
        .map(|e| {
            let (b, _) = native::read(&e).unwrap();
            let endpoint_range = range(&e);
            assert_eq!(
                capability::active(&b, &check_format(&e).unwrap()).unwrap(),
                endpoint_range
            );
            assert!(
                !has_snapshot(&e),
                "Existing user snapshot must be reviewed before this test"
            );
            let level = native::settings(&e).unwrap().level().unwrap();
            record(json!({"event":"card-original","state":capture(&e)}));
            (e, b, level)
        })
        .collect();
    assert!(!originals.is_empty());
    let mut failures = Vec::new();
    let run_matrix = (|| -> Result<()> {
        for (e, b, level) in &originals {
            let e = devices::get(&e.id)?;
            let sample = check_format(&e)?;
            let mut last = b.clone();
            for (label, all) in [("current", false), ("repeat-current", false), ("all", true)] {
                let before = devices::get(&e.id)?;
                let planned =
                    capability::plan(&last, &sample, before.rate, before.channels, 3.0, all)?;
                let status = run("apply", &before, 3.0, all);
                let seen = devices::get(&e.id)?;
                native::check_test_volume(&seen, *level)?;
                let actual = native::read(&seen)?.0;
                let ok = status.is_ok()
                    && actual == planned.bytes
                    && range(&seen) == capability::active(&planned.bytes, &sample)?;
                record(
                    json!({"event":label,"id":e.id,"ok":ok,"status":status,"state":capture(&seen)}),
                );
                for (s, orig, _) in originals.iter().filter(|(s, _, _)| s.id != e.id) {
                    let v = devices::get(&s.id)?;
                    let isolated = native::read(&v)?.0 == *orig && range(&v) == range(s);
                    record(
                        json!({"event":"sibling-during","stage":label,"target":e.id,"sibling":s.id,"ok":isolated,"state":capture(&v)}),
                    );
                    if !isolated {
                        failures.push(format!("{} sibling changed", s.name))
                    }
                }
                if !ok {
                    failures.push(format!("{} {label}", e.name));
                    break;
                }
                last = actual;
            }
            let target = devices::get(&e.id)?;
            let restored = if has_snapshot(&target) {
                run("restore", &target, 0.0, false)
            } else {
                Err("No snapshot was written".into())
            };
            let seen = devices::get(&e.id)?;
            let bytes = native::read(&seen)?.0;
            let ok =
                restored.is_ok() && bytes == *b && range(&seen) == capability::active(b, &sample)?;
            record(
                json!({"event":"restore","id":e.id,"ok":ok,"status":restored,"state":capture(&seen)}),
            );
            if !ok {
                failures.push(format!("{} restore", e.name))
            }
            if bytes != *b {
                return Err("普通恢复未还原基线字节，暂停后续设备操作。".into());
            }
            for (s, orig, _) in originals.iter().filter(|(s, _, _)| s.id != e.id) {
                let v = devices::get(&s.id)?;
                let isolated = native::read(&v)?.0 == *orig && range(&v) == range(s);
                record(
                    json!({"event":"sibling-isolation","target":e.id,"sibling":s.id,"ok":isolated,"state":capture(&v)}),
                );
                if !isolated {
                    return Err("同卡对照发生变化，暂停后续设备操作。".into());
                }
            }
        }
        let target = devices::get(&reference.id)?;
        let forced = run("force", &target, 0.0, false);
        record(json!({"event":"force","target":reference.id,"ok":forced.is_ok(),"status":forced}));
        if let Err(e) = forced {
            failures.push(e)
        }
        Ok(())
    })();
    if let Err(err) = run_matrix {
        failures.push(err)
    }
    // Read the final volume and mute state only for the confirmed connector.
    for (old, bytes, (level, mute)) in &originals {
        let restore_user = (|| -> Result<bool> {
            let now = devices::list()?;
            let unique = originals
                .iter()
                .filter(|(v, _, _)| same_connector(old, v))
                .count()
                == 1;
            let v = returned_output(old, &now, unique)?;
            native::check_test_volume(v, (*level, *mute))?;
            if old.id.eq_ignore_ascii_case(&v.id) && native::read(v)?.0 != *bytes && has_snapshot(v)
            {
                let repaired = run("restore", v, 0.0, false);
                record(json!({"event":"cleanup-restore","target":v.id,"status":repaired}));
            }
            let v = &devices::get(&v.id)?;
            native::check_test_volume(v, (*level, *mute))?;
            let (current, _) = native::read(v)?;
            // Windows can reorder entire capability rows when rebuilding.
            let as_rows = |raw: &[u8]| -> Result<Vec<Vec<u8>>> {
                let mut rows: Vec<_> = capability::parse(raw)?
                    .iter()
                    .map(|r| {
                        let mut row = r.raw.to_vec();
                        for n in [r.period.step, r.period.default, r.period.min, r.period.max] {
                            row.extend(n.to_le_bytes())
                        }
                        row
                    })
                    .collect();
                rows.sort();
                Ok(rows)
            };
            let good = as_rows(&current)? == as_rows(bytes)?
                && range(v) == range(old)
                && v.raw_format == old.raw_format;
            record(
                json!({"event":"final","oldId":old.id,"ok":good,"exactBytes":current==*bytes,"state":capture(v)}),
            );
            Ok(good)
        })();
        if restore_user != Ok(true) {
            failures.push(format!("{} final {:?}", old.name, restore_user))
        }
    }
    record(json!({"event":"end","failures":failures}));
    assert!(failures.is_empty(), "{failures:?}");
}
