use super::{capability, native, store};
use crate::devices::{self, Endpoint};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, SystemTime},
};
use windows::{
    Win32::System::Registry::*,
    core::{PCWSTR, w},
};

struct OperationNote {
    id: String,
    output_format: String,
    mix_format: String,
    data: Value,
}
static LAST: Mutex<Option<OperationNote>> = Mutex::new(None);

fn windows_codes(message: &str) -> Vec<String> {
    let mut codes = Vec::new();
    for (prefix, radix) in [("0x", 16), ("返回 ", 10), ("Windows ", 10)] {
        for (start, _) in message.match_indices(prefix) {
            let tail = &message[start + prefix.len()..];
            if radix == 10 && tail.starts_with("0x") {
                continue;
            }
            let digits: String = tail
                .chars()
                .take_while(|c| c.is_digit(radix))
                .take(9)
                .collect();
            if digits.is_empty() || digits.len() > 8 {
                continue;
            }
            let code = if radix == 16 {
                format!("0x{}", digits.to_ascii_uppercase())
            } else {
                digits
            };
            if !codes.contains(&code) {
                codes.push(code);
            }
        }
    }
    codes
}

pub fn remember_operation(e: &Endpoint, op: &str, result: &Result<String, String>, offer: bool) {
    if !matches!(op, "apply" | "restore" | "force" | "hardRestart") {
        return;
    }
    let msg = match result {
        Ok(msg) | Err(msg) => msg,
    };
    if let Ok(mut last) = LAST.lock() {
        *last = Some(OperationNote {
            id: e.id.clone(),
            output_format: e.raw_format.clone(),
            mix_format: e.mix_raw.clone(),
            data: json!({"operation":op, "ok":result.is_ok(), "reloadOffered":offer,
                "windowsCodes":windows_codes(msg)}),
        });
    }
}

fn os_dword(value: PCWSTR) -> Option<u32> {
    let mut number = 0u32;
    let mut size = 4;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            value,
            RRF_RT_REG_DWORD,
            None,
            Some((&mut number as *mut u32).cast()),
            Some(&mut size),
        )
    }
    .is_ok()
    .then_some(number)
}

fn build_number() -> Option<u32> {
    let mut chars = [0u16; 32];
    let mut size = size_of_val(&chars) as u32;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(chars.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    String::from_utf16(&chars[..chars.iter().position(|c| *c == 0)?])
        .ok()?
        .parse()
        .ok()
}

fn frames(r: capability::Range) -> Value {
    json!({"min":r.min,"default":r.default,"max":r.max,"step":r.step})
}

fn format_metadata(e: &Endpoint) -> Value {
    json!({
        "device":{"rate":e.rate,"validBits":e.bits,"containerBits":e.container_bits,"channels":e.channels,
            "encoding":e.encoding},
        "mix":{"rate":e.mix_rate,"bits":e.mix_bits,"channels":e.mix_channels,"encoding":e.mix_encoding},
        "publishedFrames":frames(super::range(e)), "currentFrames":e.current, "currentRate":e.current_rate
    })
}

fn read_error(e: &str) -> Value {
    json!({"status":"readFailed","windowsCodes":windows_codes(e)})
}

struct Sample {
    payload: Result<Vec<u8>, String>,
    persisted: Result<Vec<u8>, String>,
}
impl Sample {
    fn read(e: &Endpoint) -> Self {
        Self {
            payload: native::read_payload(e),
            persisted: store::persisted(&e.id),
        }
    }
    fn report(&self, raw: &[u8]) -> Value {
        let payload = self.payload.as_ref();
        let persisted = self.persisted.as_ref();
        let info = match payload {
            Ok(p) => {
                let rows = capability::parse(p);
                let active = capability::active(p, raw);
                json!({"bytes":p.len(),"rows":rows.as_ref().ok().map(Vec::len),"layoutError":rows.err(),
                    "formatRange":active.as_ref().ok().copied().map(frames),"formatError":active.err()})
            }
            Err(e) => read_error(e),
        };
        let disk = match persisted {
            Ok(p) => {
                json!({"bytes":p.len(),"wrapper":p.iter().take(8).copied().collect::<Vec<_>>()})
            }
            Err(e) => read_error(e),
        };
        let comparison = match (payload, persisted) {
            (Ok(p), Ok(d)) => {
                let validated = capability::verify_persisted(p, d);
                let body = d.get(8..);
                let offset = body.and_then(|b| {
                    b.iter()
                        .zip(p)
                        .position(|(a, c)| a != c)
                        .or_else(|| (b.len() != p.len()).then(|| b.len().min(p.len())))
                });
                json!({"accepted":validated.is_ok(),"reason":validated.err(),"firstBodyDifference":offset})
            }
            _ => Value::Null,
        };
        json!({"endpointProperty":info,"persistedProperty":disk,"comparison":comparison})
    }
}

// Export the device class/vendor/product without the final PnP instance or serial.
fn usb_model(instance: &str) -> Option<String> {
    let mut parts = instance.split('\\');
    if !parts.next()?.eq_ignore_ascii_case("USB") {
        return None;
    }
    let hardware = parts.next()?.to_ascii_uppercase();
    let mut keys = hardware.split('&');
    let vid = keys.next()?.strip_prefix("VID_")?;
    let pid = keys.next()?.strip_prefix("PID_")?;
    if [vid, pid]
        .iter()
        .all(|s| s.len() == 4 && s.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        Some(format!("{vid}:{pid}"))
    } else {
        None
    }
}

pub fn write_report(id: &str) -> Result<PathBuf, String> {
    let _com = native::Apartment::enter()?;
    let e = devices::get(id)?;
    let mut report = json!({
        "schemaVersion":1, "applicationVersion":env!("CARGO_PKG_VERSION"),
        "utcUnixSeconds":SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs(),
        "windows":{"build":build_number(),"revision":os_dword(w!("UBR"))},
        "output":{"usbVidPid":usb_model(&e.function_id),"driverVersion":e.driver_version,
            "driverProvider":e.driver_provider,"driverService":e.driver_service,"driverInf":e.driver_inf,
            "microsoftUac2Configurable":e.supported,"canBenchmark":e.can_benchmark,"formFactor":e.form_factor},
        "audio":format_metadata(&e),
    });
    if let Ok(last) = LAST.lock() {
        report["lastOperationThisSession"] = last
            .as_ref()
            .filter(|note| {
                note.id.eq_ignore_ascii_case(id)
                    && note.output_format == e.raw_format
                    && note.mix_format == e.mix_raw
            })
            .map(|note| note.data.clone())
            .unwrap_or(Value::Null);
    }
    if e.supported {
        let raw = capability::hex(&e.raw_format)?;
        let first = Sample::read(&e);
        std::thread::sleep(Duration::from_millis(120));
        let second = Sample::read(&e);
        let stable = match (
            &first.payload,
            &first.persisted,
            &second.payload,
            &second.persisted,
        ) {
            (Ok(a), Ok(b), Ok(c), Ok(d)) => Some(a == c && b == d),
            _ => None,
        };
        report["capabilities"] =
            json!({"stableAcrossReads":stable,"samples":[first.report(&raw),second.report(&raw)]});
        report["recovery"] = match store::load(&e) {
            Ok(Some(s)) => json!({"present":true,"reloadPending":s.reload_pending,
                "originalRange":capability::active(&s.original,&raw).ok().map(frames),
                "intendedRange":capability::active(&s.intended,&raw).ok().map(frames)}),
            Ok(None) => json!({"present":false}),
            Err(error) => read_error(&error),
        };
    }
    let after = devices::get(id)?;
    if e.arrival != after.arrival
        || e.function_id != after.function_id
        || e.physical_id != after.physical_id
        || e.raw_format != after.raw_format
        || e.mix_raw != after.mix_raw
        || e.supported != after.supported
    {
        return Err("采集期间所选设备或格式发生变化，请刷新后重新生成。".into());
    }
    report["audioAfterRead"] = format_metadata(&after);
    let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    crate::data::write_diagnostic(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_report_describes_known_and_unknown_records_without_raw_payloads() {
        let sample = Sample {
            payload: Ok(vec![0x53, 0x33]),
            persisted: Ok(vec![65, 0, 0x35, 12, 1, 0, 0, 0, 0x53, 0x33]),
        };
        let good = sample.report(&[]);
        assert_eq!(good["comparison"]["accepted"], true);
        let mismatch = Sample {
            persisted: Ok(vec![65, 0, 0x35, 12, 1, 0, 0, 0, 0x53, 0x44]),
            ..sample
        };
        let bad = mismatch.report(&[]);
        assert_eq!(bad["comparison"]["firstBodyDifference"], 1);
        assert_eq!(bad["comparison"]["accepted"], false);
        let err = read_error(r"error C:\Users\private\test 0x80070005");
        assert_eq!(err["windowsCodes"], json!(["0x80070005"]));
        assert!(!err.to_string().contains("private"));
        assert_eq!(windows_codes("重启返回 3010。"), vec!["3010"]);
        assert_eq!(
            usb_model(r"USB\VID_1234&PID_ABCD&MI_00\SERIAL-123").as_deref(),
            Some("1234:ABCD")
        );
        assert_eq!(usb_model(r"USB\VID_username&PID_foo\SERIAL"), None);
    }
}
