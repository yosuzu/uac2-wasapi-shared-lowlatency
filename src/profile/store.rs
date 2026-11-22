use super::{Result, capability};
use crate::{
    data::{self, Document},
    devices::Endpoint,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use windows::{
    Win32::{
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, HANDLE, WAIT_TIMEOUT,
            WIN32_ERROR,
        },
        System::{
            Registry::*,
            Threading::{CreateEventW, WaitForSingleObject},
        },
    },
    core::{HSTRING, Owned, PCWSTR, PWSTR},
};

const ROOT: &str = r"SOFTWARE\UAC2 Shared Period\EndpointProfiles\v1";
const PID2: &str = "{a2a3fff4-353f-407c-9d86-1f9dc7d5a606},2";

fn check(code: WIN32_ERROR, label: &str) -> Result<()> {
    code.ok()
        .map_err(|e| format!("{label}失败（Windows {}，{}）。", code.0, e.message()))
}

pub(super) fn guid(id: &str) -> Result<String> {
    let g = id
        .strip_prefix("{0.0.0.00000000}.")
        .ok_or("需要明确选择一个播放端点。")?;
    if g.len() != 38
        || !g.bytes().enumerate().all(|(i, b)| match i {
            0 => b == b'{',
            37 => b == b'}',
            9 | 14 | 19 | 24 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
    {
        return Err("播放端点 GUID 无效。".into());
    }
    Ok(g.to_ascii_lowercase())
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn read_binary(path: &str, name: &str) -> Result<Option<Vec<u8>>> {
    let mut h = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(path),
            None,
            KEY_QUERY_VALUE | KEY_WOW64_64KEY,
            &mut h,
        )
    };
    if matches!(status, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
        return Ok(None);
    }
    check(status, "打开只读注册表项")?;
    let key = Key(h);
    let name = HSTRING::from(name);
    let mut size = 0;
    let mut ty = REG_VALUE_TYPE::default();
    let code =
        unsafe { RegQueryValueExW(key.0, &name, None, Some(&mut ty), None, Some(&mut size)) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    check(code, "查询二进制值")?;
    if ty != REG_BINARY || !(1..=4 * 1024 * 1024).contains(&size) {
        return Err("受保护的记录类型或长度异常。".into());
    }
    let mut data = vec![0u8; size as usize];
    check(
        unsafe {
            RegQueryValueExW(
                key.0,
                &name,
                None,
                Some(&mut ty),
                Some(data.as_mut_ptr()),
                Some(&mut size),
            )
        },
        "读取二进制值",
    )?;
    if ty != REG_BINARY || size as usize != data.len() {
        return Err("注册表数据在读取时已变化。".into());
    }
    Ok(Some(data))
}

pub(super) fn persisted(id: &str) -> Result<Vec<u8>> {
    let path = format!(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\{}\Properties",
        guid(id)?
    );
    read_binary(&path, PID2)?.ok_or("未找到所选端点能力的持久化副本。".into())
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Snapshot {
    schema: u32,
    endpoint: String,
    physical: String,
    function: String,
    container: String,
    driver: String,
    #[serde(with = "bytes")]
    pub original: Vec<u8>,
    #[serde(with = "bytes")]
    original_registry: Vec<u8>,
    // The journal is flushed and read back before opening the audio writer.
    // After a crash either preimage or candidate is recoverable.
    #[serde(with = "bytes")]
    pub before: Vec<u8>,
    #[serde(with = "bytes")]
    pub intended: Vec<u8>,
    pub reload_pending: bool,
}
// Compact portable encoding; old HKLM JSON contains integer arrays.
mod bytes {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{
        Deserializer, Serializer,
        de::{Error, SeqAccess, Visitor},
    };
    use std::fmt;
    const MAX: usize = 1024 * 1024 + 8;
    pub fn serialize<S: Serializer>(b: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(b))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct Bytes;
        impl<'de> Visitor<'de> for Bytes {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded capability blob")
            }
            fn visit_str<E: Error>(self, s: &str) -> Result<Vec<u8>, E> {
                if s.len() > MAX.div_ceil(3) * 4 {
                    return Err(E::custom("能力记录超过上限"));
                }
                let decoded = STANDARD.decode(s).map_err(E::custom)?;
                if decoded.len() > MAX {
                    return Err(E::custom("能力记录超过上限"));
                }
                Ok(decoded)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
                let mut out = Vec::new();
                while let Some(b) = seq.next_element::<u8>()? {
                    if out.len() == MAX {
                        return Err(A::Error::custom("能力记录超过上限"));
                    }
                    out.push(b)
                }
                Ok(out)
            }
        }
        d.deserialize_any(Bytes)
    }
}
impl Snapshot {
    pub fn new(e: &Endpoint, original: Vec<u8>, registry: Vec<u8>) -> Self {
        Self {
            schema: 1,
            endpoint: e.id.clone(),
            physical: e.physical_id.clone(),
            function: e.function_id.clone(),
            container: e.container.clone(),
            driver: e.driver_version.clone(),
            before: original.clone(),
            intended: original.clone(),
            original,
            original_registry: registry,
            reload_pending: false,
        }
    }
    pub fn owns(&self, bytes: &[u8]) -> bool {
        bytes == self.original || bytes == self.before || bytes == self.intended
    }
    pub fn restore_from(&self, current: &[u8]) -> Result<Vec<u8>> {
        if !self.owns(current) {
            return Err("端点记录已由其他来源更改，无法覆盖。".into());
        }
        Ok(self.original.clone())
    }
    fn validate(&self, e: &Endpoint) -> Result<()> {
        if self.schema != 1
            || self.driver.is_empty()
            || self.driver != e.driver_version
            || [
                (&self.endpoint, &e.id),
                (&self.physical, &e.physical_id),
                (&self.function, &e.function_id),
                (&self.container, &e.container),
            ]
            .iter()
            .any(|(a, b)| b.is_empty() || !a.eq_ignore_ascii_case(b))
        {
            return Err("本地快照属于不同的端点、设备或驱动版本。".into());
        }
        capability::verify_persisted(&self.original, &self.original_registry)
            .map_err(|err| format!("快照中的注册表原件未通过核对。{err}"))?;
        if self.before.len() != self.original.len() || self.intended.len() != self.original.len() {
            return Err("快照的原件或写入记录不完整。".into());
        }
        capability::parse(&self.original)?;
        capability::parse(&self.before)?;
        capability::parse(&self.intended)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialized_journal_restores_exact_preimage_at_each_crash_boundary() {
        let fixtures: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("fixtures/capabilities.json")).unwrap();
        let e = Endpoint {
            id: "{0.0.0.00000000}.{11111111-2222-3333-4444-555555555555}".into(),
            physical_id: "usb physical".into(),
            function_id: "audio interface".into(),
            container: "container".into(),
            driver_version: "10".into(),
            ..Default::default()
        };
        let spdif = capability::hex(fixtures[3]["original"].as_str().unwrap()).unwrap();
        for f in &fixtures[..3] {
            let original = capability::hex(f["original"].as_str().unwrap()).unwrap();
            let registry = capability::hex(f["persisted"].as_str().unwrap()).unwrap();
            let candidate = capability::hex(f["candidate"].as_str().unwrap()).unwrap();
            let mut s = Snapshot::new(&e, original.clone(), registry.clone());
            s.intended = candidate.clone();
            s.reload_pending = true;
            let reopened: Snapshot =
                serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
            reopened.validate(&e).unwrap();
            assert_eq!(reopened.original_registry, registry);
            let restored = reopened.restore_from(&candidate).unwrap();
            assert_eq!(restored, original);
            capability::verify_persisted(&restored, &registry).unwrap();
            assert_eq!(reopened.restore_from(&original).unwrap(), original);
            assert!(reopened.restore_from(&spdif).is_err());
            let mismatched = Endpoint {
                physical_id: "replacement".into(),
                ..e.clone()
            };
            assert!(reopened.validate(&mismatched).is_err());
            let mut variable = reopened;
            variable.original_registry[2..4].copy_from_slice(&[0x34, 0x51]);
            let mut corrupt: Snapshot =
                serde_json::from_slice(&serde_json::to_vec(&variable).unwrap()).unwrap();
            corrupt.validate(&e).unwrap();
            assert_eq!(corrupt.original_registry, variable.original_registry);
            assert_eq!(corrupt.restore_from(&candidate).unwrap(), original);
            corrupt.original_registry[4] = 0;
            assert!(corrupt.validate(&e).is_err());
        }
    }
}

// Different operations (including 0.1.1) may write byte-identical journals.
// Arm before preparation reads the value and retain the one-shot notification
// until the confirmation ends. Closing the key cancels it before the event drops.
pub(super) struct LegacyWatch {
    _key: Key,
    event: Owned<HANDLE>,
}

impl LegacyWatch {
    fn arm() -> Result<Option<Self>> {
        let mut h = HKEY::default();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                &HSTRING::from(ROOT),
                None,
                KEY_NOTIFY | KEY_WOW64_64KEY,
                &mut h,
            )
        };
        if matches!(status, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
            return Ok(None);
        }
        check(status, "打开旧版设备快照通知")?;
        let key = Key(h);
        let event = unsafe {
            Owned::new(
                CreateEventW(None, true, false, PCWSTR::null())
                    .map_err(|e| format!("建立待重载记录通知失败：{e}"))?,
            )
        };
        check(
            unsafe {
                RegNotifyChangeKeyValue(
                    key.0,
                    false,
                    REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC,
                    Some(*event),
                    true,
                )
            },
            "监视已有快照的更改",
        )?;
        Ok(Some(Self { _key: key, event }))
    }
    pub fn unchanged(&self) -> Result<()> {
        if unsafe { WaitForSingleObject(*self.event, 0) } != WAIT_TIMEOUT {
            return Err("确认期间快照记录已更新或无法复核，请重新确认硬重启。".into());
        }
        Ok(())
    }
}

fn legacy(e: &Endpoint) -> Result<Option<Vec<u8>>> {
    read_binary(ROOT, &guid(&e.id)?)
}
fn table(doc: &Map<String, Value>) -> Result<Option<&Map<String, Value>>> {
    doc.get("endpoints")
        .map(|x| x.as_object().ok_or("端点恢复表已损坏。".into()))
        .transpose()
}
fn table_mut(doc: &mut Map<String, Value>) -> Result<&mut Map<String, Value>> {
    doc.entry("endpoints")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("端点恢复表已损坏。".into())
}
fn lookup<'a>(doc: Option<&'a Map<String, Value>>, id: &str) -> Result<Option<&'a Value>> {
    Ok(doc
        .map(table)
        .transpose()?
        .flatten()
        .and_then(|x| x.get(id)))
}
fn verify_record(
    e: &Endpoint,
    record: &Value,
    machine: &str,
    legacy: Option<&[u8]>,
) -> Result<Snapshot> {
    if record.get("machine").and_then(Value::as_str) != Some(machine) {
        return Err("此端点的恢复记录属于其他 Windows 安装，已保留原件。".into());
    }
    if let Some(old) = legacy {
        if record.get("legacyDigest").and_then(Value::as_str) != Some(&data::digest(old)?) {
            return Err("旧版工具已改动此端点的恢复记录，已保留两份原件，请先核对。".into());
        }
    }
    let s: Snapshot = serde_json::from_value(
        record
            .get("snapshot")
            .ok_or("本地恢复原件不完整。")?
            .clone(),
    )
    .map_err(|e| e.to_string())?;
    s.validate(e)?;
    Ok(s)
}
fn old_snapshot(e: &Endpoint, bytes: &[u8]) -> Result<Snapshot> {
    let s: Snapshot = serde_json::from_slice(bytes).map_err(|_| "旧版原始快照无法解析。")?;
    s.validate(e)?;
    Ok(s)
}
pub(super) fn load(e: &Endpoint) -> Result<Option<Snapshot>> {
    let id = guid(&e.id)?;
    let doc = data::read(Document::Recovery)?;
    let old = legacy(e)?;
    if let Some(record) = lookup(doc.as_ref(), &id)? {
        return verify_record(e, record, &data::machine()?, old.as_deref()).map(Some);
    }
    old.as_deref()
        .map(|bytes| old_snapshot(e, bytes))
        .transpose()
}
fn merge_record(
    doc: &mut Map<String, Value>,
    e: &Endpoint,
    s: &Snapshot,
    machine: &str,
    legacy: Option<&[u8]>,
) -> Result<()> {
    let id = guid(&e.id)?;
    s.validate(e)?;
    let records = table_mut(doc)?;
    if records.len() >= 256 && !records.contains_key(&id) {
        return Err("端点恢复表已达上限，已有原件均已保留。".into());
    }
    let existing = records
        .get(&id)
        .map(|r| verify_record(e, r, machine, legacy))
        .transpose()?;
    if let Some(prior) = existing.or(legacy.map(|b| old_snapshot(e, b)).transpose()?) {
        if prior.original != s.original || prior.original_registry != s.original_registry {
            return Err("首次修改前的原件发生冲突，已保留原件。".into());
        }
    }
    let record = records
        .entry(id)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("无效的端点恢复记录。")?;
    record.insert("machine".into(), json!(machine));
    if let Some(old) = legacy {
        record.insert("legacyDigest".into(), json!(data::digest(old)?));
    }
    let fields = serde_json::to_value(s).map_err(|e| e.to_string())?;
    let snap = record
        .entry("snapshot")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("无效的恢复快照。")?;
    snap.extend(fields.as_object().ok_or("恢复原件无法序列化。")?.clone());
    doc.insert("revision".into(), json!(data::revision()?));
    Ok(())
}

pub(super) fn save(e: &Endpoint, s: &Snapshot) -> Result<()> {
    let old = legacy(e)?;
    let machine = data::machine()?;
    data::update(Document::Recovery, |doc| {
        merge_record(doc, e, s, &machine, old.as_deref())
    })
}

fn revision() -> Result<Option<String>> {
    match data::read(Document::Recovery)? {
        Some(doc) => doc
            .get("revision")
            .and_then(Value::as_str)
            .map(|s| Some(s.to_owned()))
            .ok_or("恢复文件修订号缺失。".into()),
        None => Ok(None),
    }
}
pub(super) struct JournalWatch {
    revision: Option<String>,
    old: Option<LegacyWatch>,
}
impl JournalWatch {
    pub fn arm() -> Result<Self> {
        Ok(Self {
            revision: revision()?,
            old: LegacyWatch::arm()?,
        })
    }
    pub fn unchanged(&self) -> Result<()> {
        if revision()? != self.revision {
            return Err("硬重启确认期间本地恢复文件已更新，请重新确认。".into());
        }
        match &self.old {
            Some(w) => w.unchanged(),
            None if LegacyWatch::arm()?.is_some() => {
                Err("硬重启确认期间出现了旧版快照，请重新确认。".into())
            }
            None => Ok(()),
        }
    }
}
pub(super) fn finish_pending(
    e: &Endpoint,
    expected: &Snapshot,
    watch: &JournalWatch,
) -> Result<()> {
    let mut snap = load(e)?.ok_or("原待重载记录已不存在，请刷新。")?;
    if !snap.reload_pending || &snap != expected {
        return Err("待重载目标已改变，请刷新。".into());
    }
    watch.unchanged()?;
    snap.before.clone_from(&snap.intended);
    snap.reload_pending = false;
    save(e, &snap)
}

// Import even disconnected endpoints. Validate the original against both
// saved preimages now, and match the complete live identity again before use.
pub fn migrate_legacy_snapshots() -> Result<usize> {
    let _lock = super::native::Operation::lock()?;
    let mut h = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(ROOT),
            None,
            KEY_QUERY_VALUE | KEY_WOW64_64KEY,
            &mut h,
        )
    };
    if matches!(status, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
        return Ok(0);
    }
    check(status, "读取旧版端点清单")?;
    let key = Key(h);
    let current = data::read(Document::Recovery)?;
    let mut batch = Vec::new();
    let mut total = 0usize;
    for i in 0..=256 {
        let mut name = [0u16; 128];
        let mut len = name.len() as u32;
        let status = unsafe {
            RegEnumValueW(
                key.0,
                i,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        check(status, "枚举旧版端点")?;
        if i == 256 {
            return Err("旧版端点数量超出导入上限，已保留原件。".into());
        }
        let name = String::from_utf16(&name[..len as usize]).map_err(|e| e.to_string())?;
        let id = format!("{{0.0.0.00000000}}.{name}");
        let normalized = guid(&id)?;
        if lookup(current.as_ref(), &normalized)?.is_some() {
            continue;
        }
        let bytes = read_binary(ROOT, &name)?.ok_or("旧版端点在读取期间已改变。")?;
        total = total
            .checked_add(bytes.len())
            .ok_or("旧版恢复记录大小异常。")?;
        if total > 32 * 1024 * 1024 {
            return Err("旧版恢复记录超过迁移上限，已保留原件。".into());
        }
        let s: Snapshot = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let e = Endpoint {
            id,
            physical_id: s.physical.clone(),
            function_id: s.function.clone(),
            container: s.container.clone(),
            driver_version: s.driver.clone(),
            ..Default::default()
        };
        s.validate(&e)?;
        batch.push((e, s, bytes));
    }
    if !batch.is_empty() {
        let machine = data::machine()?;
        data::update(Document::Recovery, |doc| {
            for (e, s, bytes) in &batch {
                merge_record(doc, e, s, &machine, Some(bytes))?;
            }
            Ok(())
        })?;
    }
    Ok(batch.len())
}

#[cfg(test)]
pub(super) fn remove_verified_test_snapshot(e: &Endpoint, original: &[u8]) -> Result<bool> {
    let _lock = super::native::Operation::lock()?;
    if legacy(e)?.is_some() {
        return Ok(false);
    }
    let id = guid(&e.id)?;
    let Some(actual) = load(e)? else {
        return Ok(false);
    };
    let expected = Snapshot::new(e, original.to_vec(), actual.original_registry.clone());
    if actual != expected {
        return Err("已保留未完全恢复的验收记录。".into());
    }
    data::update(Document::Recovery, |doc| {
        let records = table_mut(doc)?;
        let record = records.get(&id).ok_or("验收快照已改变。")?;
        if verify_record(e, record, &data::machine()?, None)? != expected {
            return Err("验收快照已改变。".into());
        }
        records.remove(&id);
        doc.insert("revision".into(), json!(data::revision()?));
        Ok(())
    })?;
    Ok(true)
}

#[cfg(test)]
mod portable_tests {
    use super::*;
    #[test]
    fn legacy_arrays_roundtrip_preserves_first_image_unknown_keys_and_device_binding() {
        let fixtures: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/capabilities.json")).unwrap();
        let original = capability::hex(fixtures[0]["original"].as_str().unwrap()).unwrap();
        let registry = capability::hex(fixtures[0]["persisted"].as_str().unwrap()).unwrap();
        let candidate = capability::hex(fixtures[0]["candidate"].as_str().unwrap()).unwrap();
        let e = Endpoint {
            id: "{0.0.0.00000000}.{11111111-2222-3333-4444-555555555555}".into(),
            physical_id: "fixture-USB".into(),
            function_id: "fixture-output".into(),
            container: "fixture-box".into(),
            driver_version: "fixture-1".into(),
            ..Default::default()
        };
        let s = Snapshot::new(&e, original.clone(), registry.clone());
        let mut legacy = serde_json::to_value(&s).unwrap();
        for key in ["original", "before", "intended"] {
            legacy[key] = json!(&original);
        }
        legacy["original_registry"] = json!(&registry);
        let bytes = serde_json::to_vec(&legacy).unwrap();
        let mut inferred = old_snapshot(&e, &bytes).unwrap();
        let mut doc = Map::from_iter([("future".into(), json!({"keep":1}))]);
        merge_record(&mut doc, &e, &inferred, "fixture-machine", Some(&bytes)).unwrap();
        let id = guid(&e.id).unwrap();
        let record = &mut table_mut(&mut doc).unwrap()[&id];
        record["snapshot"]["future"] = json!(17);
        inferred.intended = candidate.clone();
        inferred.reload_pending = true;
        merge_record(&mut doc, &e, &inferred, "fixture-machine", Some(&bytes)).unwrap();
        let saved = lookup(Some(&doc), &id).unwrap().unwrap();
        assert!(saved["snapshot"]["original"].is_string());
        assert_eq!(saved["snapshot"]["future"], 17);
        assert_eq!(doc["future"]["keep"], 1);
        let again = verify_record(&e, saved, "fixture-machine", Some(&bytes)).unwrap();
        assert_eq!(again.restore_from(&candidate).unwrap(), original);
        assert!(verify_record(&e, saved, "another-machine", None).is_err());
        assert!(
            verify_record(
                &e,
                saved,
                "fixture-machine",
                Some(b"changed legacy journal")
            )
            .is_err()
        );
        inferred.original = candidate;
        assert!(merge_record(&mut doc, &e, &inferred, "fixture-machine", None).is_err());
    }
}
