mod activity;
mod capability;
#[cfg(test)]
mod compatibility_tests;
pub mod diagnostics;
#[cfg(test)]
mod live;
mod native;
mod port;
mod store;

pub use activity::force_blocked_message;
pub use store::migrate_legacy_snapshots;

use crate::devices::{self, Endpoint};
use std::{
    thread,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, String>;

pub fn has_snapshot(e: &Endpoint) -> bool {
    e.supported && store::load(e).ok().flatten().is_some()
}

// Called on a full endpoint inspection. Never open a hub to draw the button.
pub fn snapshot_status(e: &Endpoint) -> (bool, bool) {
    if !e.supported {
        return (false, false);
    }
    let Ok(Some(snapshot)) = store::load(e) else {
        return (false, false);
    };
    let persisted = snapshot.reload_pending
        && native::read(e)
            .and_then(|(actual, _)| pending_range(e, &snapshot, &actual))
            .is_ok();
    (true, persisted)
}

fn pending_range(
    e: &Endpoint,
    snapshot: &store::Snapshot,
    actual: &[u8],
) -> Result<capability::Range> {
    if !snapshot.reload_pending || snapshot.intended != actual {
        return Err("没有已写入并与本次待重载目标一致的端点记录。".into());
    }
    capability::active(actual, &check_format(e)?)
}

/// This is the original minimum in this PC's first pre-edit snapshot.
pub fn original_min(e: &Endpoint) -> Option<u32> {
    if !e.supported {
        return None;
    }
    let s = store::load(e).ok()??;
    capability::active(&s.original, &capability::hex(&e.raw_format).ok()?)
        .ok()
        .map(|r| r.min)
}

fn range(e: &Endpoint) -> capability::Range {
    capability::Range {
        step: e.fundamental,
        default: e.default,
        min: e.min,
        max: e.max,
    }
}

fn same_function(a: &Endpoint, b: &Endpoint) -> bool {
    !a.physical_id.is_empty()
        && a.physical_id.eq_ignore_ascii_case(&b.physical_id)
        && !a.function_id.is_empty()
        && a.function_id.eq_ignore_ascii_case(&b.function_id)
        && !a.container.is_empty()
        && a.container.eq_ignore_ascii_case(&b.container)
}

fn fresh(expected: &Endpoint) -> Result<Endpoint> {
    let e = devices::get(&expected.id)?;
    if !devices::matches_target(expected, &e) || !valid_usb_path(&e) {
        return Err("设备、驱动、格式或本次 USB 连接已经变化，请刷新并重新确认。".into());
    }
    Ok(e)
}

fn valid_usb_path(e: &Endpoint) -> bool {
    native::is_usb_id(&e.function_id)
        && native::is_usb_id(&e.physical_id)
        && e.physical_id
            .split('\\')
            .nth(1)
            .is_some_and(|hardware| !hardware.to_ascii_uppercase().contains("&MI_"))
}

fn fresh_rebuild(expected: &Endpoint) -> Result<Endpoint> {
    let e = devices::get_for_rebuild(&expected.id)?;
    if !devices::matches_rebuild_target(expected, &e) || !valid_usb_path(&e) {
        return Err(
            "无法核对原微软 USB 音频功能、驱动、本次连接或同卡输出，请刷新并重新确认。".into(),
        );
    }
    Ok(e)
}

fn check_format(e: &Endpoint) -> Result<Vec<u8>> {
    if e.rate == 0 || e.channels == 0 || e.rate != e.mix_rate || e.channels != e.mix_channels {
        return Err("无法核对当前设备和共享混音的采样率、声道数。".into());
    }
    capability::hex(&e.raw_format)
}

struct Sibling {
    id: String,
    name: String,
    bytes: Option<Vec<u8>>,
    period: capability::Range,
    raw: String,
}
fn siblings(e: &Endpoint) -> Result<Vec<Sibling>> {
    Ok(devices::list()?
        .iter()
        .filter(|x| !x.id.eq_ignore_ascii_case(&e.id) && same_function(e, x))
        .map(|x| Sibling {
            id: x.id.clone(),
            name: x.name.clone(),
            bytes: native::read(x).ok().map(|(b, _)| b),
            period: range(x),
            raw: x.raw_format.clone(),
        })
        .collect())
}
fn siblings_report(e: &Endpoint, old: &[Sibling]) -> String {
    if old.is_empty() {
        return String::new();
    }
    let current = devices::list().unwrap_or_default();
    let mut result = Vec::new();
    for s in old {
        let v = current
            .iter()
            .find(|v| v.supported && same_function(v, e) && v.id.eq_ignore_ascii_case(&s.id));
        let msg = match (v, &s.bytes, v.and_then(|v| native::read(v).ok())) {
            (Some(v), Some(before), Some((now, _)))
                if *before == now
                    && range(v) == s.period
                    && v.raw_format.eq_ignore_ascii_case(&s.raw) =>
            {
                "记录、格式及公布范围一致"
            }
            (Some(_), Some(_), Some(_)) => "记录、格式或公布范围有变化",
            _ => "待复核",
        };
        result.push(format!("{}：{}", s.name, msg));
    }
    format!("同卡对照：{}。", result.join("；"))
}

fn after_reload(
    e: &Endpoint,
    desired: &[u8],
    expected_range: capability::Range,
) -> Result<Endpoint> {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let attempt = devices::get(&e.id).and_then(|v| {
            if !v.supported
                || !same_function(e, &v)
                || v.driver_version != e.driver_version
                || !e.raw_format.eq_ignore_ascii_case(&v.raw_format)
                || e.mix_rate != v.mix_rate
                || e.mix_channels != v.mix_channels
            {
                return Err("目标设备、驱动或格式发生变化。".into());
            }
            if native::read(&v)?.0 != desired {
                return Err("能力记录尚未与写入值一致。".into());
            }
            if range(&v) != expected_range {
                return Err(format!(
                    "Windows 实读最小 {} 帧，默认 {} 帧；期望最小 {} 帧，默认 {} 帧。",
                    v.min, v.default, expected_range.min, expected_range.default
                ));
            }
            Ok(v)
        });
        if attempt.is_ok() || Instant::now() >= deadline {
            return attempt;
        }
        thread::sleep(Duration::from_millis(320));
    }
}

fn validate_published_range(
    e: &Endpoint,
    raw: &[u8],
    current: &[u8],
    snapshot: &store::Snapshot,
) -> Result<()> {
    if !snapshot.owns(current) {
        return Err("所选端点记录已由其他来源更改，无法与本工具的快照或候选相符。".into());
    }
    let published = range(e);
    if capability::active(current, raw)? == published {
        return Ok(());
    }
    if snapshot.reload_pending
        && [&snapshot.original, &snapshot.before, &snapshot.intended]
            .iter()
            .any(|stage| capability::active(stage, raw).ok() == Some(published))
    {
        return Ok(());
    }
    Err("当前格式的能力行和 Windows 公布范围不符，也不属于已知的待重载阶段。".into())
}

fn confirm_write_target(e: &Endpoint) -> Result<()> {
    if range(&fresh(e)?) != range(e) {
        return Err("写入前 Windows 公布范围又发生了变化，请刷新后核对。".into());
    }
    Ok(())
}

fn configure(
    action: &str,
    e: &Endpoint,
    ms: f64,
    all: bool,
    offer: &mut bool,
    restart: impl FnOnce(&Endpoint) -> Result<u32>,
) -> Result<String> {
    let raw = check_format(e)?;
    let usb_arrival = native::arrival(&e.physical_id)?;
    let (current, registry) =
        native::read(e).map_err(|err| format!("本次修改尚未写入，读取端点原件失败。{err}"))?;
    let existing = store::load(e)?;
    let mut snapshot = match (action, existing) {
        ("restore", None) => return Err("所选端点没有与本机和驱动匹配的首次修改前快照。".into()),
        (_, Some(s)) => s,
        (_, None) => store::Snapshot::new(e, current.clone(), registry.clone()),
    };
    validate_published_range(e, &raw, &current, &snapshot)?;
    let (desired, description) = if action == "restore" {
        (
            snapshot.restore_from(&current)?,
            "恢复首次修改前的快照".to_string(),
        )
    } else {
        let p = capability::plan(&current, &raw, e.rate, e.channels, ms, all)?;
        (
            p.bytes,
            format!("配置 {} 个采样率的 {} 条 PCM 记录", p.rates.len(), p.rows),
        )
    };
    let published = capability::active(&desired, &raw)?;
    let changed = current != desired;
    let pending = changed || snapshot.reload_pending || published != range(e);
    if !pending {
        return Ok(format!(
            "所选端点已符合目标。Windows 实读最小 {} 帧，默认 {} 帧。",
            e.min, e.default
        ));
    }
    let comparisons = siblings(e)?;
    let previous_settings = native::settings(e)?;
    confirm_write_target(e)?;
    if native::arrival(&e.physical_id)? != usb_arrival {
        return Err("物理 USB 连接已改变。".into());
    }
    // Retrying an already persisted candidate must retain the still-running
    // preimage until Windows has actually reloaded it.
    if !snapshot.reload_pending || current != snapshot.intended || desired != snapshot.intended {
        snapshot.before = current.clone();
    }
    snapshot.intended = desired.clone();
    snapshot.reload_pending = true;
    store::save(e, &snapshot)?;
    confirm_write_target(e)?;
    native::write(e, &current, &registry, &desired)?;
    fresh(e).map_err(|error| format!("能力记录已写入并读回，重载前核对失败：{error}"))?;
    if native::arrival(&e.physical_id)? != usb_arrival {
        return Err("记录已写入，物理连接发生变化，请刷新。".into());
    }
    let code = restart(e)
        .inspect_err(|_| *offer = true)
        .map_err(|err| format!("能力记录已写入并读回，重载待复核。{err} 可单独确认「硬重启」，或重新插拔此声卡、重启电脑后刷新。"))?;
    let verified=after_reload(e,&desired,published).inspect_err(|_| *offer = true).map_err(|err|
        format!("端点记录已写入并独立读回，设备重启返回 {code}。{err} 可单独确认「硬重启」，或重新插拔此声卡、重启电脑后刷新核对。"))?;
    // A matching active format alone cannot confirm a restart for the other
    // formats. Keep the retry journal intact when Windows deferred the restart.
    if code != 0 {
        *offer = true;
        return Err(format!(
            "记录已写入；当前格式独立实读下限 {} 帧，默认 {} 帧。设备重启返回 {code}。可单独确认「硬重启」，或重新插拔此声卡、重启电脑后刷新核对。",
            verified.min, verified.default
        ));
    }
    snapshot.before = desired;
    snapshot.reload_pending = false;
    store::save(&verified, &snapshot)?;
    let settings = native::settings(&verified)?;
    Ok(format!(
        "已{description}。当前 {} Hz 公布最小 {} 帧，默认 {} 帧，步进 {} 帧。开流情况请在试流验证中查看。{}{}",
        verified.rate,
        verified.min,
        verified.default,
        verified.fundamental,
        previous_settings.report(&settings, &e.id, &verified.id),
        siblings_report(&verified, &comparisons)
    ))
}

/// A new endpoint ID may be accepted only for a unique old and new connector.
/// A shared association or a common USB container alone never selects an output.
fn same_connector(a: &Endpoint, b: &Endpoint) -> bool {
    same_function(a, b)
        && a.form_factor != u32::MAX
        && a.form_factor == b.form_factor
        && !a.association.is_empty()
        && a.association.eq_ignore_ascii_case(&b.association)
}
fn returned_output<'a>(
    e: &Endpoint,
    outputs: &'a [Endpoint],
    unique_before: bool,
) -> Result<&'a Endpoint> {
    let same = |v: &Endpoint| same_function(e, v) && devices::same_rebuild_driver(e, v);
    if let Some(old) = outputs.iter().find(|v| v.id.eq_ignore_ascii_case(&e.id)) {
        if same(old)
            && old.form_factor == e.form_factor
            && old.association.eq_ignore_ascii_case(&e.association)
        {
            return Ok(old);
        }
        return Err("原端点 ID 的 USB 功能、连接器或驱动身份发生变化。".into());
    }
    let mut candidates = outputs.iter().filter(|v| same(v) && same_connector(e, v));
    if unique_before
        && let Some(single) = candidates.next()
        && candidates.next().is_none()
    {
        return Ok(single);
    }
    Err("原输出未重新上线，或新端点无法唯一识别。".into())
}
pub fn rediscover(before: &Endpoint, old: &[Endpoint], now: &[Endpoint]) -> Option<String> {
    if !old
        .iter()
        .any(|e| devices::matches_rebuild_target(before, e))
    {
        return None;
    }
    let unique = old.iter().filter(|e| same_connector(before, e)).count() == 1;
    returned_output(before, now, unique)
        .ok()
        .map(|e| e.id.clone())
}

/// The UI keeps this object in native memory until the user confirms or cancels.
/// Consuming it releases the registry notification and prevents duplicate use.
pub struct HardRestart {
    endpoint: Endpoint,
    port: port::PortTarget,
    snapshot: store::Snapshot,
    watch: store::JournalWatch,
    unique_before: bool,
    prepared: Instant,
}

impl HardRestart {
    pub fn hub(&self) -> &str {
        &self.port.hub_instance
    }
    pub fn port(&self) -> u32 {
        self.port.port
    }

    fn recheck(&self) -> Result<()> {
        if self.prepared.elapsed() >= Duration::from_secs(120) {
            return Err("硬重启确认已过期，请重新确认。".into());
        }
        let now = fresh(&self.endpoint)?;
        if range(&now) != range(&self.endpoint) {
            return Err("已公布周期在确认期间发生变化，请刷新。".into());
        }
        if store::load(&now)?.as_ref() != Some(&self.snapshot) {
            return Err("原待重载记录已改变，请重新确认。".into());
        }
        pending_range(&now, &self.snapshot, &native::read(&now)?.0)?;
        self.watch.unchanged()
    }

    fn read_returned(&self) -> Result<Endpoint> {
        let old = &self.endpoint;
        let now = devices::get(&old.id)?;
        if !now.supported
            || !same_function(old, &now)
            || old.driver_version != now.driver_version
            || !old.id.eq_ignore_ascii_case(&now.id)
            || old.form_factor != now.form_factor
            || !old.association.eq_ignore_ascii_case(&now.association)
        {
            return Err("原端点、USB 功能、连接器或驱动身份尚未核对。".into());
        }
        if now.arrival == old.arrival || now.arrival == 0 {
            return Err("尚未读到音频功能重新枚举后的到达时间。".into());
        }
        let arrival = port::returned(&now, &self.port)?;
        if now.raw_format != old.raw_format || now.mix_raw != old.mix_raw {
            return Err("重枚举后的设备格式或共享混音格式已改变。".into());
        }
        let expected = pending_range(&now, &self.snapshot, &native::read(&now)?.0)?;
        verify_running_range(&now, expected)?;
        if devices::formats(&now.id)? != (now.raw_format.clone(), now.mix_raw.clone())
            || native::arrival(&now.physical_id)? != arrival
            || native::arrival(&now.function_id)? != now.arrival
        {
            return Err("读回期间，设备格式或连接又发生了变化。".into());
        }
        Ok(now)
    }

    fn changed_id(&self) -> Option<String> {
        let now = devices::list().ok()?;
        let e = returned_output(&self.endpoint, &now, self.unique_before).ok()?;
        if !e.supported
            || self.endpoint.id.eq_ignore_ascii_case(&e.id)
            || e.driver_version != self.endpoint.driver_version
            || e.arrival == 0
            || e.arrival == self.endpoint.arrival
        {
            return None;
        }
        port::returned(e, &self.port).ok()?;
        Some(format!(
            "原连接器上的输出「{}」使用了新的端点 ID {}。原快照仍绑定 {}，请重新选择并核对。",
            e.name, e.id, self.endpoint.id
        ))
    }
}

fn verify_running_range(e: &Endpoint, expected: capability::Range) -> Result<()> {
    if range(e) != expected {
        return Err(format!(
            "Windows 实读最小 {} 帧，默认 {} 帧，步进 {} 帧，最大 {} 帧；目标为 {}/{}/{}/{} 帧。",
            e.min,
            e.default,
            e.fundamental,
            e.max,
            expected.min,
            expected.default,
            expected.step,
            expected.max
        ));
    }
    if e.current_rate != e.mix_rate
        || !(expected.min..=expected.max).contains(&e.current)
        || !e.current.is_multiple_of(expected.step)
    {
        return Err("尚未读到与当前混音格式和公布范围一致的运行周期。".into());
    }
    Ok(())
}

pub fn prepare_hard_restart(expected: &Endpoint) -> Result<HardRestart> {
    let _operation = native::Operation::lock()?;
    let _apartment = native::Apartment::enter()?;
    let watch = store::JournalWatch::arm()?;
    let e = fresh(expected)?;
    if range(&e) != range(expected) {
        return Err("已公布周期发生变化，请刷新后重新确认。".into());
    }
    let snapshot = store::load(&e)?.ok_or("所选输出没有待重载的快照。")?;
    pending_range(&e, &snapshot, &native::read(&e)?.0)?;
    let outputs = devices::list()?;
    let unique_before = outputs.iter().filter(|v| same_connector(&e, v)).count() == 1;
    let port = port::unique_target(&e)?;
    let plan = HardRestart {
        endpoint: e,
        port,
        snapshot,
        watch,
        unique_before,
        prepared: Instant::now(),
    };
    plan.recheck()?;
    Ok(plan)
}

pub fn hard_restart(plan: HardRestart) -> Result<String> {
    let _operation = native::Operation::lock()?;
    let _apartment = native::Apartment::enter()?;
    plan.recheck()?;
    port::cycle_present(&plan.endpoint, &plan.port, || plan.recheck())?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        plan.watch.unchanged()?;
        match plan.read_returned() {
            Ok(e) => {
                store::finish_pending(&e, &plan.snapshot, &plan.watch).map_err(|err| {
                    format!("端点已重新枚举，周期已核对，但写回快照状态失败。{err}")
                })?;
                return Ok(format!(
                    "硬重启完成。{} 的 USB 端口 {} 已重新枚举，所选端点全部能力记录已核对。当前格式 {} Hz，Windows 公布最小 {} 帧，默认 {} 帧，步进 {} 帧，最大 {} 帧；当前 {} 帧。",
                    e.name,
                    plan.port(),
                    e.rate,
                    e.min,
                    e.default,
                    e.fundamental,
                    e.max,
                    e.current
                ));
            }
            Err(reason) if Instant::now() >= deadline => {
                let detail = plan.changed_id().unwrap_or(reason);
                return Err(format!("已请求一次 USB 端口重连，原端点仍待复核。{detail}"));
            }
            Err(_) => thread::sleep(Duration::from_millis(350)),
        }
    }
}

fn rebuilt_range_report(v: &Endpoint) -> String {
    if v.supported
        && v.rate > 0
        && v.mix_rate == v.rate
        && v.min > 0
        && v.default >= v.min
        && v.max >= v.default
        && v.fundamental > 0
    {
        format!(
            "当前 {} Hz，Windows 公布最小 {} 帧，默认 {} 帧，步进 {} 帧，最大 {} 帧。",
            v.rate, v.min, v.default, v.fundamental, v.max
        )
    } else {
        "尚未核对当前格式的共享周期，可刷新或生成诊断报告。".into()
    }
}

pub struct ForceResult {
    pub endpoint: Endpoint,
    pub message: String,
    pub snapshot_status: &'static str,
}

fn read_rebuilt_snapshot(
    before: &Endpoint,
    returned: &Endpoint,
    unique_before: bool,
) -> Result<(Endpoint, Vec<u8>, Vec<u8>)> {
    let outputs = devices::list_for_rebuild()?;
    let e = returned_output(before, &outputs, unique_before)?;
    if !returned.id.eq_ignore_ascii_case(&e.id) || returned.arrival != e.arrival {
        return Err("已返回的端点或 USB 连接又发生了变化。".into());
    }
    if !e.supported {
        return Err(format!("输出的格式或共享周期尚未就绪。{}", e.reason));
    }
    let raw = check_format(e)?;
    let arrival = native::arrival(&e.physical_id)?;
    let (current, registry) = native::read(e)?;
    if capability::active(&current, &raw)? != range(e) {
        return Err("重建后的能力记录与 Windows 公布范围尚未一致。".into());
    }
    let checked = devices::get_for_rebuild(&e.id)?;
    if !devices::matches_target(e, &checked)
        || !devices::matches_rebuild_target(e, &checked)
        || range(e) != range(&checked)
    {
        return Err("读取重建快照期间，设备身份、格式或公布周期发生了变化。".into());
    }
    let (again, persisted) = native::read(&checked)?;
    native::verify_preimage(&current, &registry, &again, &persisted)?;
    if native::arrival(&e.function_id)? != e.arrival || native::arrival(&e.physical_id)? != arrival
    {
        return Err("读回期间 USB 连接发生了变化。".into());
    }
    Ok((checked, current, registry))
}

fn removal_failure(
    error: &str,
    present: Result<bool>,
    original_output_online: impl FnOnce() -> bool,
) -> String {
    let advice = match present {
        Ok(false) => "原 USB 音频功能目前不在场。请稍候刷新，如果声卡仍未返回，请重新插拔声卡或重启电脑。".to_owned(),
        Ok(true) if original_output_online() => "原 USB 音频功能和所选输出目前仍在线。请关闭使用这块声卡的程序，刷新并核对输出后再决定是否重试。".to_owned(),
        Ok(true) => "原 USB 音频功能仍在场，所选输出的原连接状态尚未核实。请稍候刷新，如果声卡仍不可用，请重新插拔声卡或重启电脑。".to_owned(),
        Err(reason) => format!("当前 USB 音频功能状态也未能核实。{reason} 请稍候刷新，如果声卡仍不可用，请重新插拔声卡或重启电脑。"),
    };
    format!("{error} {advice}")
}

const REMOVE_REBOOT: (&str, &str) = (
    "卸载所选 USB 音频功能返回 3010。Windows 已接受操作，需要重启电脑才能完成。请先保存工作，重启 Windows 后刷新并核对所选输出。",
    "Removing the selected USB audio function returned 3010. Windows accepted the operation and requires a restart to complete it. Save your work, restart Windows, then refresh and check the selected output.",
);
const SCAN_REBOOT: (&str, &str) = (
    "卸载所选 USB 音频功能返回 0，随后定向扫描返回 3010。Windows 已接受扫描，需要重启电脑才能完成。请先保存工作，重启 Windows 后刷新并核对所选输出。",
    "Removing the selected USB audio function returned 0. The subsequent targeted scan returned 3010. Windows accepted the scan and requires a restart to complete it. Save your work, restart Windows, then refresh and check the selected output.",
);

pub fn force_reboot_message(error: &str, en: bool) -> Option<String> {
    [REMOVE_REBOOT, SCAN_REBOOT]
        .into_iter()
        .find(|(zh, _)| error == *zh)
        .map(|(zh, english)| if en { english } else { zh }.to_owned())
}

fn stop_on_required_reboot(result: &Result<u32>, message: (&str, &str)) -> Result<()> {
    if matches!(result, Ok(3010)) {
        return Err(message.0.into());
    }
    Ok(())
}

fn force(e: &Endpoint) -> Result<ForceResult> {
    let previous = native::settings(e).ok();
    let old_outputs = devices::list_for_rebuild()?;
    if !old_outputs
        .iter()
        .any(|v| devices::matches_rebuild_target(e, v))
    {
        return Err("无法在操作前的活动输出中再次核对所选端点，请刷新后重试。".into());
    }
    let unique_before = old_outputs.iter().filter(|v| same_connector(e, v)).count() == 1;
    let physical_arrival = native::arrival(&e.physical_id)?;
    let port = port::target(e);
    let checked = fresh_rebuild(e)?;
    if checked.siblings != e.siblings {
        return Err("同卡端点列表已经变化，请重新确认。".into());
    }
    native::started(&e.physical_id)?;
    if native::arrival(&e.physical_id)? != physical_arrival {
        return Err("物理 USB 连接已改变。".into());
    }
    activity::require_idle(&checked)?;
    if native::arrival(&checked.function_id)? != checked.arrival
        || native::arrival(&checked.physical_id)? != physical_arrival
    {
        return Err("核对音频占用期间 USB 连接已改变，请刷新并重新确认。".into());
    }
    let command = native::scoped_command(&["/remove-device", &checked.function_id]);
    stop_on_required_reboot(&command, REMOVE_REBOOT)?;
    let code = match command {
        Ok(0) => 0,
        other => {
            let error = match other {
                Ok(code) => format!("卸载所选 USB 音频功能返回 {code}。"),
                Err(reason) => format!("卸载所选 USB 音频功能时未取得返回码。{reason}"),
            };
            return Err(removal_failure(
                &error,
                native::function_present(&checked.function_id),
                || {
                    native::arrival(&checked.physical_id) == Ok(physical_arrival)
                        && fresh_rebuild(&checked).is_ok()
                },
            ));
        }
    };
    let mut scan_note = String::new();
    if !e.physical_id.eq_ignore_ascii_case(&e.function_id)
        && native::arrival(&e.physical_id) == Ok(physical_arrival)
    {
        let scan = native::scoped_command(&["/scan-devices", "/instanceid", &e.physical_id]);
        stop_on_required_reboot(&scan, SCAN_REBOOT)?;
        match scan {
            Ok(0) => {}
            Ok(code) => scan_note = format!("定向扫描返回 {code}。"),
            Err(err) => scan_note = err,
        }
    }
    let began = Instant::now();
    let mut deadline = began + Duration::from_secs(16);
    let mut fallback = Some(port);
    loop {
        let attempt: Result<Endpoint> = (|| {
            let outputs = devices::list_for_rebuild()?;
            let v = returned_output(e, &outputs, unique_before)?;
            if v.arrival == 0 || v.arrival == e.arrival && v.id.eq_ignore_ascii_case(&e.id) {
                return Err("尚未观察到音频功能重新识别。".into());
            }
            native::started(&v.physical_id)?;
            native::started(&v.function_id)?;
            native::started(&format!(r"SWD\MMDEVAPI\{}", v.id))?;
            Ok(v.clone())
        })();
        if let Ok(mut returned) = attempt {
            // PnP can finish before the audio engine and registry publish the
            // new table. Once the output is back, only retry read-only probes.
            let deadline = Instant::now() + Duration::from_secs(12);
            let saved = loop {
                match read_rebuilt_snapshot(e, &returned, unique_before) {
                    Ok((verified, current, registry)) => {
                        returned = verified;
                        break store::save_rebuilt(&returned, current, registry);
                    }
                    Err(err) if Instant::now() >= deadline => break Err(err),
                    Err(_) => thread::sleep(Duration::from_millis(400)),
                }
            };
            let (snapshot_status, snapshot_report) = match saved {
                Ok(store::RebuiltSnapshot::Saved) => (
                    "saved",
                    "已保存并读回 Windows 重建后的默认设置快照。".into(),
                ),
                Ok(store::RebuiltSnapshot::Preserved) => (
                    "preserved",
                    "已核对并保留本机已有的首次快照，可用于普通恢复。".into(),
                ),
                Err(err) => ("unavailable", format!("普通恢复快照未能就绪。{err}")),
            };
            let readback = rebuilt_range_report(&returned);
            let comparison = previous
                .as_ref()
                .zip(native::settings(&returned).ok().as_ref())
                .map(|(old, new)| old.report(new, &e.id, &returned.id))
                .unwrap_or_else(|| "默认播放角色、音量及静音未能完整对照。".into());
            let message = format!(
                "Windows 已重新识别 {}，微软 USB 音频驱动 {}。{snapshot_report}{readback}{comparison}{scan_note}",
                returned.name, returned.driver_version
            );
            return Ok(ForceResult {
                endpoint: returned,
                message,
                snapshot_status,
            });
        }
        if began.elapsed() >= Duration::from_secs(5)
            && let Some(port) = fallback.take()
        {
            match port.and_then(|port| port::reconnect(e, &port)) {
                Ok(()) => {
                    scan_note.push_str("已对原端口执行一次软件重连。");
                    deadline = Instant::now() + Duration::from_secs(16);
                }
                Err(reason) => {
                    scan_note.push_str(&format!("未进行端口重连：{reason} "));
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "卸载音频功能返回 {code}。{scan_note}{} 请重插此声卡或重启电脑，再刷新并核对输出。",
                attempt.unwrap_err()
            ));
        }
        thread::sleep(Duration::from_millis(400));
    }
}

pub fn run_force(expected: &Endpoint) -> Result<ForceResult> {
    let _operation = native::Operation::lock()?;
    let _apartment = native::Apartment::enter()?;
    force(&fresh_rebuild(expected)?)
}

pub fn run(action: &str, expected: &Endpoint, ms: f64, all: bool) -> Result<String> {
    run_with_restart_offer(action, expected, ms, all, &mut false)
}

pub fn run_with_restart_offer(
    action: &str,
    expected: &Endpoint,
    ms: f64,
    all: bool,
    offer: &mut bool,
) -> Result<String> {
    *offer = false;
    if action == "force" {
        return run_force(expected).map(|r| r.message);
    }
    let _operation = native::Operation::lock()?;
    let _apartment = native::Apartment::enter()?;
    let e = fresh(expected)?;
    if expected.siblings != e.siblings {
        return Err("同卡设备列表在确认后发生了变化。".into());
    }
    match action {
        "apply" | "restore" => {
            if range(expected) != range(&e) {
                return Err("已公布周期在确认后发生变化，请先刷新。".into());
            }
            configure(action, &e, ms, all, offer, |target| {
                native::scoped_command(&["/restart-device", &target.physical_id])
            }).map_err(|err| format!("{err} 可生成诊断报告。若需让 Windows 重新识别设备，可另行确认红色「强制恢复」。"))
        }
        _ => Err("未知的设备操作。".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_reboot_receipt_stops_both_remove_and_scan_before_reconnect_or_snapshot() {
        for message in [REMOVE_REBOOT, SCAN_REBOOT] {
            let mut continued = false;
            let result = stop_on_required_reboot(&Ok(3010), message).map(|()| continued = true);
            let error = result.unwrap_err();
            assert!(!continued);
            assert!(error.contains("3010") && error.contains("重启 Windows"));
            assert_eq!(force_reboot_message(&error, true).unwrap(), message.1);
            assert!(force_blocked_message(&error, false).is_none());
            for other in [Ok(0), Ok(5), Err("no exit code".into())] {
                assert!(stop_on_required_reboot(&other, message).is_ok());
            }
        }
        assert!(force_reboot_message("设备目前仍在线。", true).is_none());
    }

    #[test]
    fn a_remove_error_reports_the_actual_code_and_observed_device_state() {
        let error = "卸载所选 USB 音频功能返回 9876。";
        let online = removal_failure(error, Ok(true), || true);
        assert!(online.contains("9876") && online.contains("所选输出目前仍在线"));
        assert!(removal_failure(error, Ok(true), || false).contains("原连接状态尚未核实"));
        assert!(removal_failure(error, Ok(false), || panic!()).contains("目前不在场"));
        assert!(removal_failure(error, Err("CM 0x42".into()), || panic!()).contains("CM 0x42"));
    }

    #[test]
    fn old_win10_journal_requires_the_entire_persisted_candidate() {
        let (original, registry) = compatibility_tests::win10_capture();
        let e = Endpoint {
            raw_format:
                "FEFF020080BB000000DC05000800200016002000030000000100000000001000800000AA00389B71"
                    .into(),
            rate: 48000,
            mix_rate: 48000,
            channels: 2,
            mix_channels: 2,
            ..Endpoint::default()
        };
        let mut s = store::Snapshot::new(&e, original.clone(), registry);
        let raw = check_format(&e).unwrap();
        let candidate = capability::plan(&original, &raw, 48000, 2, 3.0, true)
            .unwrap()
            .bytes;
        s.intended = candidate.clone();
        assert!(pending_range(&e, &s, &candidate).is_err());
        s.reload_pending = true;
        assert!(pending_range(&e, &s, &original).is_err());
        let partial = capability::plan(&original, &raw, 48000, 2, 3.0, false)
            .unwrap()
            .bytes;
        assert_eq!(capability::active(&partial, &raw).unwrap().min, 144);
        assert!(pending_range(&e, &s, &partial).is_err());
        let expected = pending_range(&e, &s, &candidate).unwrap();
        assert_eq!(expected.min, 144);
        let running = Endpoint {
            min: 144,
            default: 480,
            fundamental: 1,
            max: 480,
            current: 480,
            current_rate: 48000,
            ..e.clone()
        };
        assert!(verify_running_range(&running, expected).is_ok());
        for wrong in [
            Endpoint {
                min: 336,
                ..running.clone()
            },
            Endpoint {
                current: 0,
                ..running.clone()
            },
            Endpoint {
                max: 960,
                ..running.clone()
            },
            Endpoint {
                current_rate: 44100,
                ..running
            },
        ] {
            assert!(verify_running_range(&wrong, expected).is_err());
        }
        // A deferred restore may have already persisted precisely the original.
        s.before = candidate;
        s.intended = original.clone();
        assert_eq!(pending_range(&e, &s, &original).unwrap().min, 336);
    }
    #[test]
    fn reenumeration_never_follows_default_spdif_or_an_ambiguous_connector() {
        let e = Endpoint {
            id: "old headphone".into(),
            arrival: 10,
            supported: true,
            can_rebuild: true,
            driver_service: "usbaudio2".into(),
            driver_provider: "Microsoft".into(),
            driver_inf: "usbaudio2.inf".into(),
            driver_version: "10.0".into(),
            physical_id: "card".into(),
            container: "shared".into(),
            function_id: "wave".into(),
            association: "wave link".into(),
            form_factor: 3,
            ..Default::default()
        };
        let headphone = Endpoint {
            id: "new headphone".into(),
            arrival: 11,
            ..e.clone()
        };
        let spdif = Endpoint {
            id: "new spdif".into(),
            form_factor: 8,
            is_default: true,
            ..headphone.clone()
        };
        let other = Endpoint {
            id: "other".into(),
            physical_id: "other card".into(),
            ..headphone.clone()
        };
        let returned = [spdif.clone(), other, headphone.clone()];
        assert_eq!(
            returned_output(&e, &returned, true).unwrap().id,
            "new headphone"
        );
        assert!(returned_output(&e, &returned, false).is_err());
        assert!(returned_output(&e, &returned[..2], true).is_err());
        let twin = Endpoint {
            id: "second headphone".into(),
            ..headphone.clone()
        };
        assert!(returned_output(&e, &[headphone, twin], true).is_err());
        assert!(
            returned_output(
                &e,
                &[Endpoint {
                    id: e.id.clone(),
                    ..spdif
                }],
                true
            )
            .is_err()
        );
    }
}
