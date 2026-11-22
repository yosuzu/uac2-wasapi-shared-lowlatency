//! Version and layout observed in the three hardware captures. All accesses to
//! untrusted lengths are bounded before a row can participate in a write.
use std::collections::BTreeSet;

const DEFAULT_MODE: &[u8] = &[
    32, 234, 144, 158, 147, 180, 209, 79, 161, 168, 126, 19, 97, 169, 86, 207,
];
const PCM: &[u8] = &[1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113];
const COMMON: [u32; 8] = [44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Range {
    pub step: u32,
    pub default: u32,
    pub min: u32,
    pub max: u32,
}
pub(super) struct Row<'a> {
    pub raw: &'a [u8],
    pub rate: u32,
    pub channels: u16,
    eligible: bool,
    pub period: Range,
    offset: usize,
}

fn word(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
fn dword(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

pub(super) fn hex(raw: &str) -> Result<Vec<u8>, String> {
    if raw.len() % 2 != 0 || !raw.is_ascii() {
        return Err("设备格式的十六进制编码无效。".into());
    }
    raw.as_bytes()
        .chunks_exact(2)
        .map(|b| {
            u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16)
                .map_err(|_| "设备格式编码无效。".into())
        })
        .collect()
}

pub(super) fn parse(bytes: &[u8]) -> Result<Vec<Row<'_>>, String> {
    if bytes.len() < 28
        || dword(bytes, 0) != 10
        || dword(bytes, 4) != 1
        || &bytes[8..24] != DEFAULT_MODE
    {
        return Err("所选端点的能力版本或处理模式尚未验证。".into());
    }
    let count = dword(bytes, 24) as usize;
    if count == 0 || count > 1024 {
        return Err("端点格式行数异常。".into());
    }
    let mut rows = Vec::with_capacity(count);
    let mut i = 28;
    for _ in 0..count {
        let header = bytes.get(i..i + 38).ok_or("端点格式记录不完整。")?;
        let extra = word(header, 36) as usize;
        let end = i + 38 + extra;
        let w = bytes.get(i + 20..end).ok_or("端点格式数据长度不符。")?;
        let rate = dword(w, 4);
        let channels = word(w, 2);
        let bits = word(w, 14);
        let tag = word(w, 0);
        let (pcm, valid) = match (tag, extra) {
            (1, 0) => (true, bits),
            (0xfffe, 22) => (&w[24..40] == PCM, word(w, 18)),
            (0xfffe, 0..22) => return Err("扩展 PCM 记录不完整。".into()),
            _ => (false, 0),
        };
        let eligible = pcm && matches!((valid, bits), (16, 16) | (24, 24) | (24, 32) | (32, 32));
        let period = Range {
            step: dword(header, 0),
            default: dword(header, 4),
            min: dword(header, 8),
            max: dword(header, 12),
        };
        if eligible {
            let align = u32::from(channels) * u32::from(bits / 8);
            if channels == 0
                || rate == 0
                || align != u32::from(word(w, 12))
                || rate.checked_mul(align) != Some(dword(w, 8))
                || period.step == 0
                || period.min == 0
                || !(period.min..=period.max).contains(&period.default)
                || period.max != dword(header, 16)
                || [period.min, period.default, period.max]
                    .iter()
                    .any(|v| v % period.step != 0)
            {
                return Err("PCM 记录的帧范围、采样率或对齐方式无法确认。".into());
            }
        }
        rows.push(Row {
            raw: w,
            rate,
            channels,
            eligible,
            period,
            offset: i + 8,
        });
        i = end;
    }
    if i != bytes.len() {
        return Err("端点能力记录末尾含未知数据。".into());
    }
    Ok(rows)
}

pub(super) fn active(bytes: &[u8], raw: &[u8]) -> Result<Range, String> {
    let found: Vec<_> = parse(bytes)?
        .into_iter()
        .filter(|r| r.eligible && r.raw == raw)
        .collect();
    match found.as_slice() {
        [] => Err("记录中没有与 Windows 当前输出格式完全相同的已知 PCM 行。".into()),
        [first, ..] if found.iter().all(|r| r.period == first.period) => Ok(first.period),
        _ => Err("相同设备格式对应不同周期，无法唯一确认。".into()),
    }
}

/// A regenerated table can be adopted into an existing restore journal only
/// when the full set of rows differs only by row order or known PCM minima.
pub(super) fn same_except_minima(original: &[u8], rebuilt: &[u8]) -> Result<bool, String> {
    fn normalized_rows(bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut normalized = Vec::new();
        for row in parse(bytes)? {
            let start = row.offset - 8;
            let mut whole = bytes[start..start + 20 + row.raw.len()].to_vec();
            if row.eligible {
                whole[8..12].fill(0);
            }
            normalized.push(whole);
        }
        normalized.sort_unstable();
        Ok(normalized)
    }
    Ok(original.get(..28) == rebuilt.get(..28)
        && normalized_rows(original)? == normalized_rows(rebuilt)?)
}

pub(super) struct Candidate {
    pub bytes: Vec<u8>,
    pub rates: BTreeSet<u32>,
    pub rows: usize,
}
pub(super) fn plan(
    bytes: &[u8],
    raw: &[u8],
    rate: u32,
    channels: u16,
    ms: f64,
    all: bool,
) -> Result<Candidate, String> {
    let rows = parse(bytes)?;
    if !ms.is_finite() || !(0.0 < ms && ms <= 1000.0) {
        return Err("目标毫秒数无效。".into());
    }
    active(bytes, raw)?;
    if !rows
        .iter()
        .any(|r| r.eligible && r.raw == raw && r.channels == channels && r.rate == rate)
    {
        return Err("当前设备格式与共享引擎的采样率或声道数不匹配。".into());
    }
    let mut out = Candidate {
        bytes: bytes.to_vec(),
        rates: BTreeSet::new(),
        rows: 0,
    };
    for r in rows.into_iter().filter(|r| {
        r.eligible
            && r.channels == channels
            && if all {
                COMMON.contains(&r.rate)
            } else {
                r.rate == rate
            }
    }) {
        let frames = (f64::from(r.rate) * ms / 1000.0).ceil() as u32;
        let target = frames
            .div_ceil(r.period.step)
            .checked_mul(r.period.step)
            .filter(|v| *v > 0 && *v <= r.period.default)
            .ok_or_else(|| format!("{} Hz 的目标超过该格式的原默认周期。", r.rate))?;
        out.bytes[r.offset..r.offset + 4].copy_from_slice(&target.to_le_bytes());
        out.rows += 1;
        out.rates.insert(r.rate);
    }
    if out.rows == 0 {
        return Err("所选范围内没有可验证的 PCM 记录。".into());
    }
    Ok(out)
}

/// The two bytes at offsets 2 and 3 vary across observed Windows records.
pub(super) fn verify_persisted(payload: &[u8], registry: &[u8]) -> Result<(), String> {
    let Some((header, body)) = registry.split_at_checked(8) else {
        return Err("注册表能力记录的 8 字节头不完整。".into());
    };
    if !matches!(header, [0x41, 0, _, _, 1, 0, 0, 0]) {
        return Err("注册表能力记录的头部格式尚未识别。".into());
    }
    if payload.is_empty() || body.len() != payload.len() {
        return Err("注册表能力正文长度与音频接口不符。".into());
    }
    if body != payload {
        return Err("注册表能力正文与音频接口返回的内容不同。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn fixtures() -> Vec<Value> {
        serde_json::from_str(include_str!("fixtures/capabilities.json")).unwrap()
    }

    #[test]
    fn rebuilt_tables_accept_reordered_rows_and_minima_but_preserve_duplicate_and_opaque_rows() {
        let f = &fixtures()[0];
        let original = hex(f["original"].as_str().unwrap()).unwrap();
        let candidate = hex(f["candidate"].as_str().unwrap()).unwrap();
        let split = |blob: &[u8]| {
            parse(blob)
                .unwrap()
                .into_iter()
                .map(|r| {
                    let start = r.offset - 8;
                    blob[start..start + 20 + r.raw.len()].to_vec()
                })
                .collect::<Vec<_>>()
        };
        let assemble = |rows: &[Vec<u8>]| {
            let mut b = original[..28].to_vec();
            b[24..28].copy_from_slice(&(rows.len() as u32).to_le_bytes());
            b.extend(rows.concat());
            b
        };
        let mut rows = split(&candidate);
        rows.reverse();
        let reordered = assemble(&rows);
        assert!(same_except_minima(&original, &reordered).unwrap());
        rows[1] = rows[0].clone();
        assert!(!same_except_minima(&original, &assemble(&rows)).unwrap());

        let mut opaque = split(&original)[0].clone();
        opaque[20..22].copy_from_slice(&3u16.to_le_bytes()); // Float row is read-only.
        let base = assemble(&[opaque.clone()]);
        for offset in [8, 16] {
            let mut changed = opaque.clone();
            changed[offset] ^= 1;
            assert!(!same_except_minima(&base, &assemble(&[changed])).unwrap());
        }
    }

    #[test]
    fn capture_candidates_match_and_spdif_is_independent() {
        let fixtures = fixtures();
        let spdif = hex(fixtures[3]["original"].as_str().unwrap()).unwrap();
        for f in &fixtures[..3] {
            let input = hex(f["original"].as_str().unwrap()).unwrap();
            let original = input.clone();
            let format = hex(f["format"].as_str().unwrap()).unwrap();
            let rate = f["rate"].as_u64().unwrap() as u32;
            verify_persisted(&original, &hex(f["persisted"].as_str().unwrap()).unwrap()).unwrap();
            let p = plan(&input, &format, rate, 2, 3.0, false).unwrap();
            assert_eq!(p.bytes, hex(f["candidate"].as_str().unwrap()).unwrap());
            assert_eq!(
                active(&original, &format).unwrap().min,
                f["range"]["Min"].as_u64().unwrap() as u32
            );
            let all = plan(&input, &format, rate, 2, 3.0, true).unwrap();
            let (old, new) = (parse(&original).unwrap(), parse(&all.bytes).unwrap());
            assert!(all.rates.len() > 1);
            for (a, b) in old.iter().zip(new.iter()) {
                assert_eq!(a.raw, b.raw);
                assert_eq!(
                    (a.period.default, a.period.max, a.period.step),
                    (b.period.default, b.period.max, b.period.step)
                );
                if a.eligible && COMMON.contains(&a.rate) {
                    assert_eq!(b.period.min, (a.rate * 3).div_ceil(1000))
                } else {
                    assert_eq!(a.period, b.period)
                }
            }
            assert_eq!(original, input);
        }
        let spdif_wrapped = hex(fixtures[3]["persisted"].as_str().unwrap()).unwrap();
        verify_persisted(&spdif, &spdif_wrapped).unwrap();
        assert_ne!(fixtures[2]["original"], fixtures[3]["original"]);
    }

    #[test]
    fn rejects_truncation_unknown_layout_and_checks_fundamental() {
        let fixture = fixtures().remove(2);
        let orig = hex(fixture["original"].as_str().unwrap()).unwrap();
        let format = hex(fixture["format"].as_str().unwrap()).unwrap();
        for cut in 0..orig.len() {
            assert!(parse(&orig[..cut]).is_err())
        }
        let mut unknown = orig.clone();
        unknown[8] ^= 1;
        assert!(plan(&unknown, &format, 48000, 2, 3.0, false).is_err());
        assert!(plan(&orig, &format, 44100, 2, 3.0, false).is_err());
        for ms in [f64::NAN, -1.0, 11.0] {
            assert!(plan(&orig, &format, 48000, 2, ms, false).is_err())
        }
        let mut aligned = orig;
        aligned[28..32].copy_from_slice(&8u32.to_le_bytes());
        let p = plan(&aligned, &format, 48000, 2, 3.1, false).unwrap();
        assert_eq!(active(&p.bytes, &format).unwrap().min, 152);
    }
}
