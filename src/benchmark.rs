//! Silent BASS/WASAPI playback in the dedicated, unelevated benchmark process.
mod library;

use crate::devices::{self, Endpoint};
use library::Audio;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const ROUNDS: usize = 10;
const SAMPLES: usize = 97;
pub(crate) const TOTAL: usize = ROUNDS * 3;
const CANCEL: &str = "试流已取消。";
static WORKER: Mutex<()> = Mutex::new(());

pub fn validate_dir(path: &Path) -> Result<PathBuf, String> {
    library::validate_dir(path)
}

#[derive(Clone, Copy)]
enum Band {
    Default,
    Seven,
    Current,
}

impl Band {
    fn key(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Seven => "seven",
            Self::Current => "current",
        }
    }
    fn request(self, d: &Endpoint) -> f32 {
        match self {
            Self::Default if explicit_ten(d).is_some() => 0.010,
            Self::Default => 0.0,
            Self::Seven => 0.007,
            Self::Current => f32::from_bits(1),
        }
    }
    fn targets(self, d: &Endpoint) -> Vec<u32> {
        match self {
            Self::Default => explicit_ten(d).unwrap_or_else(|| vec![d.default]),
            Self::Current => vec![d.min],
            Self::Seven => valid_targets(7, d.mix_rate, d.min, d.max, d.fundamental),
        }
    }
}

fn explicit_ten(d: &Endpoint) -> Option<Vec<u32>> {
    let ten = valid_targets(10, d.mix_rate, d.min, d.max, d.fundamental);
    (!ten.is_empty() && !ten.contains(&d.default)).then_some(ten)
}

fn valid_targets(ms: u32, rate: u32, min: u32, max: u32, step: u32) -> Vec<u32> {
    // 44.1 kHz gives 308.7 frames. Historical streams actually ran at 308 or 309.
    // Accept either whole-frame rounding only if the driver advertises that period.
    let numerator = u64::from(rate) * u64::from(ms);
    let lo = (numerator / 1000) as u32;
    let hi = numerator.div_ceil(1000) as u32;
    let mut valid = Vec::new();
    for frames in [lo, hi] {
        if frames != 0
            && step != 0
            && (min..=max).contains(&frames)
            && frames % step == 0
            && valid.last() != Some(&frames)
        {
            valid.push(frames);
        }
    }
    valid
}

pub(crate) fn verify_condition(before: &Endpoint) -> Result<Endpoint, String> {
    let after = devices::get(&before.id)?;
    if !devices::matches_benchmark_target(before, &after) {
        return Err("所选输出、格式或公布的周期设置发生变化。".into());
    }
    Ok(after)
}

fn cancel_if_requested(cancelled: &impl Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err(CANCEL.into())
    } else {
        Ok(())
    }
}

fn median(samples: &[f64]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut ordered = samples.to_vec();
    ordered.sort_by(f64::total_cmp);
    let middle = ordered.len() / 2;
    Some(if ordered.len() % 2 == 0 {
        (ordered[middle - 1] + ordered[middle]) * 0.5
    } else {
        ordered[middle]
    })
}

fn frames_ms(frames: u32, rate: u32) -> f64 {
    f64::from(frames) * 1000.0 / f64::from(rate)
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Round {
    frames: Option<u32>,
    rate: Option<u32>,
    channels: Option<u32>,
    callback_frames: Option<u32>,
    callbacks: u32,
    buffer_bytes: Option<u32>,
    samples: usize,
    queue_ms: Option<f64>,
    reason: Option<String>,
}

fn measure(
    round: &mut Round,
    audio: &mut Audio,
    device: &Endpoint,
    band: Band,
    targets: &[u32],
    sequence: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    cancel_if_requested(cancelled)?;
    // Resolve the endpoint ID again for every initialization. Never follow the default.
    let index = audio.find_output(&device.id)?;
    let info = audio.open(index, band.request(device))?;
    round.rate = Some(info.rate);
    round.channels = Some(info.channels);
    round.buffer_bytes = Some(info.buffer_bytes);
    if info.rate != device.mix_rate || info.channels != u32::from(device.mix_channels) {
        return Err(format!(
            "播放流格式已改变（{} Hz，{} 声道）。",
            info.rate, info.channels
        ));
    }
    let frame_bytes = info
        .channels
        .checked_mul(4)
        .filter(|&n| n != 0)
        .ok_or("播放流声道数无效。")?;
    // Let the first, possibly oversized buffer fill before observing steady callbacks.
    for _ in 0..10 {
        cancel_if_requested(cancelled)?;
        std::thread::sleep(Duration::from_millis(25));
    }
    let (rate, frames) = devices::period(&device.id)?;
    round.frames = Some(frames);
    if rate != info.rate || !targets.contains(&frames) {
        return Err(format!(
            "Windows 实读 {frames} 帧 / {rate} Hz，与该档请求不符。"
        ));
    }
    let cycles_before = library::start_callback_window();
    let mut samples = Vec::with_capacity(SAMPLES);
    // Vary sampling phase independently of the engine tick (the same schedule across tiers).
    let mut jitter = (sequence as u32 + 1) * 7919;
    for i in 0..SAMPLES {
        cancel_if_requested(cancelled)?;
        jitter = jitter.wrapping_mul(1664525).wrapping_add(1013904223);
        std::thread::sleep(Duration::from_micros(3000 + u64::from(jitter % 12001)));
        let bytes = audio.queue_bytes()?;
        samples.push(f64::from(bytes) * 1000.0 / (f64::from(info.rate) * f64::from(frame_bytes)));
        round.samples = samples.len();
        if (i == SAMPLES / 2 || i == SAMPLES - 1) && devices::period(&device.id)? != (rate, frames)
        {
            return Err("采样期间 Windows 当前周期或采样率发生变化。".into());
        }
    }
    let (cycles, bytes) = library::callback_counts();
    round.callbacks = cycles.wrapping_sub(cycles_before);
    if round.callbacks < 3 || bytes == u32::MAX || bytes % frame_bytes != 0 {
        return Err("有效静音回调不足，无法核实播放周期。".into());
    }
    let callback_frames = bytes / frame_bytes;
    round.callback_frames = Some(callback_frames);
    if callback_frames != frames {
        return Err(format!(
            "Windows 实读 {frames} 帧，回调最短为 {callback_frames} 帧。"
        ));
    }
    round.queue_ms = median(&samples);
    Ok(())
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct BandResult {
    requested_frames: Vec<u32>,
    frames: Option<u32>,
    period_ms: Option<f64>,
    queue_ms: Option<f64>,
    rounds: Vec<Round>,
    reason: Option<String>,
}

impl BandResult {
    fn summarize(&mut self, rate: u32, interrupted: Option<&str>) {
        if self.reason.is_some() {
            return;
        }
        let complete: Vec<_> = self
            .rounds
            .iter()
            .filter_map(|r| Some((r.frames?, r.queue_ms?)))
            .collect();
        if complete.len() != ROUNDS {
            let detail = self
                .rounds
                .iter()
                .find_map(|r| r.reason.as_deref())
                .or(interrupted)
                .unwrap_or("尚未完成此档的全部轮次。");
            self.reason = Some(format!("{} / {ROUNDS} 轮有效。{detail}", complete.len()));
            return;
        }
        let frames = complete[0].0;
        if complete.iter().any(|&(f, _)| f != frames) {
            self.reason = Some(format!("{ROUNDS} 轮的实际周期不同，无法合并。"));
            return;
        }
        self.frames = Some(frames);
        self.period_ms = Some(frames_ms(frames, rate));
        self.queue_ms = median(&complete.iter().map(|&(_, ms)| ms).collect::<Vec<_>>());
    }
}

fn comparison(
    control: &BandResult,
    current: &BandResult,
    comparable: bool,
) -> (Option<f64>, &'static str) {
    match (control.queue_ms, current.queue_ms) {
        (Some(_), Some(_)) if comparable && control.frames == current.frames => {
            (None, "samePeriod")
        }
        (Some(a), Some(b)) if comparable => (Some(a - b), "difference"),
        _ => (None, "unavailable"),
    }
}

pub fn run(
    device: &Endpoint,
    dir: &Path,
    mut progress: impl FnMut(Value),
    cancelled: impl Fn() -> bool,
) -> Result<Value, String> {
    let _guard = WORKER.try_lock().map_err(|_| "另一项试流正在进行。")?;
    progress(json!({"done":0,"total":TOTAL,"stage":"prepare"}));
    if !device.can_benchmark
        || device.mix_rate == 0
        || device.mix_channels == 0
        || device.min == 0
        || device.fundamental == 0
        || device.default < device.min
        || device.max < device.default
    {
        return Err("没有读到完整的共享格式和周期范围，请刷新。".into());
    }
    verify_condition(device)?;
    let mut done = 0;
    let mut skipped = 0;
    let mut stopped = None;
    let mut cleanup_error = None;
    let mut versions = None;
    let bands = [Band::Default, Band::Seven, Band::Current];
    let mut results: [BandResult; 3] = bands.map(|b| BandResult {
        requested_frames: b.targets(device),
        ..Default::default()
    });
    if !cancelled() {
        let mut audio =
            Audio::new(dir, device.mix_rate).map_err(|e| match verify_condition(device) {
                Ok(_) => e,
                Err(reason) => format!("{e} 设置待复核：{reason}"),
            })?;
        versions = Some(json!({"bass":audio.versions[0],"basswasapi":audio.versions[1]}));
        'sample: for (band, result) in bands.into_iter().zip(&mut results) {
            if cancelled() {
                break;
            }
            if result.requested_frames.is_empty() {
                result.reason = Some("该输出在当前格式下没有可用的 7 ms 整帧周期。".into());
                skipped += ROUNDS;
                progress(
                    json!({"stage":band.key(),"done":done,"total":TOTAL,"skipped":skipped,"reason":result.reason}),
                );
                continue;
            }
            for sequence in 0..ROUNDS {
                if cancelled() {
                    break 'sample;
                }
                progress(
                    json!({"stage":band.key(),"round":sequence+1,"done":done,"total":TOTAL,"skipped":skipped}),
                );
                let mut round = Round::default();
                if let Err(e) = measure(
                    &mut round,
                    &mut audio,
                    device,
                    band,
                    &result.requested_frames,
                    sequence,
                    &cancelled,
                ) {
                    round.reason = Some(e);
                }
                if let Err(e) = audio.close_stream() {
                    cleanup_error = Some(e.clone());
                    stopped = Some(e);
                }
                // Closing the stream lets other clients determine the idle engine period.
                // Only identity, format and the published period range must remain stable.
                if let Err(e) = verify_condition(device) {
                    stopped = Some(e);
                }
                if let Some(e) = &stopped {
                    round.queue_ms = None;
                    round.reason = Some(e.clone());
                }
                done += 1;
                result.rounds.push(round);
                progress(
                    json!({"stage":band.key(),"round":sequence+1,"done":done,"total":TOTAL,"skipped":skipped}),
                );
                if stopped.is_some() {
                    break 'sample;
                }
            }
        }
        if let Err(e) = audio.close() {
            cleanup_error = Some(e);
        }
    }
    progress(json!({"stage":"verify","done":done,"total":TOTAL,"skipped":skipped}));
    let after = verify_condition(device);
    let restore_reason = cleanup_error.or_else(|| after.as_ref().err().cloned());
    let restored = restore_reason.is_none();
    let was_cancelled = cancelled();
    if was_cancelled && stopped.is_none() {
        stopped = Some(CANCEL.into());
    }
    for result in &mut results {
        result.summarize(device.mix_rate, stopped.as_deref());
    }
    let [default, seven, current] = results;
    let comparable = restored && stopped.is_none();
    let (default_delta, default_comparison) = comparison(&default, &current, comparable);
    let (seven_delta, seven_comparison) = comparison(&seven, &current, comparable);
    progress(json!({"stage":"complete","done":done,"total":TOTAL,"skipped":skipped}));
    Ok(json!({
        "currentMs":current.queue_ms,"defaultMs":default.queue_ms,"sevenMs":seven.queue_ms,"explicitTen":explicit_ten(device).is_some(),
        "defaultDelta":default_delta,"sevenDelta":seven_delta,"defaultComparison":default_comparison,
        "sevenComparison":seven_comparison,"restored":restored,"restoreReason":restore_reason,
        "cancelled":was_cancelled,"reason":stopped,"versions":versions,"condition":device,
        "afterCurrent":after.ok().map(|d| d.current),
        "bands":{"default":default,"seven":seven,"current":current}
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_ms_respects_actual_formats_and_one_frame_rounding() {
        assert_eq!(valid_targets(7, 44_100, 133, 441, 1), [308, 309]);
        assert_eq!(valid_targets(7, 44_100, 309, 441, 1), [309]);
        assert_eq!(valid_targets(7, 48_000, 144, 480, 1), [336]);
        assert!(valid_targets(7, 48_000, 384, 480, 1).is_empty());
        assert!(valid_targets(7, 48_000, 240, 480, 240).is_empty());
    }

    #[test]
    fn non_ten_default_uses_explicit_ten_only_when_advertised() {
        let mut d = Endpoint {
            mix_rate: 48000,
            min: 144,
            default: 384,
            max: 480,
            fundamental: 1,
            ..Default::default()
        };
        assert_eq!(Band::Default.request(&d), 0.010);
        assert_eq!(Band::Default.targets(&d), [480]);
        d.max = 384;
        assert_eq!(Band::Default.request(&d), 0.0);
        assert_eq!(Band::Default.targets(&d), [384]);
        d.max = 480;
        d.default = 480;
        assert_eq!(Band::Default.request(&d), 0.0);
        assert_eq!(Band::Default.targets(&d), [480]);
    }

    fn band(data: &[(u32, f64)]) -> BandResult {
        let mut result = BandResult {
            rounds: data
                .iter()
                .map(|&(f, t)| Round {
                    frames: Some(f),
                    queue_ms: Some(t),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        result.summarize(48_000, None);
        result
    }
    #[test]
    fn requires_ten_verified_rounds_at_one_period_and_uses_the_even_median() {
        let mut rounds = [(144, 0.); 10];
        for (i, entry) in rounds.iter_mut().enumerate() {
            entry.1 = (i + 1) as f64;
        }
        rounds[9].1 = 300.;
        assert_eq!(TOTAL, 30);
        assert_eq!(band(&rounds).queue_ms, Some(5.5));
        assert!(band(&rounds[..9]).queue_ms.is_none());
        rounds[9].0 = 143;
        assert!(band(&rounds).queue_ms.is_none());
    }
    #[test]
    fn comparisons_preserve_regressions_and_explain_coincident_periods() {
        let min = band(&[(144, 5.); ROUNDS]);
        let slower = band(&[(480, 3.); ROUNDS]);
        assert_eq!(comparison(&slower, &min, true), (Some(-2.), "difference"));
        assert_eq!(comparison(&min, &min, true), (None, "samePeriod"));
        assert_eq!(comparison(&slower, &min, false), (None, "unavailable"));
    }
}
