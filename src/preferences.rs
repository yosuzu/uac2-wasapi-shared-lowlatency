use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;
use uac2_control::data::{self, Document};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND},
        System::Registry::*,
    },
    core::w,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub accepted: bool,
    pub theme: String,
    pub lang: String,
    pub directory: Option<PathBuf>,
    #[serde(skip)]
    prior: Map<String, Value>,
    #[serde(skip)]
    scope: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            accepted: false,
            theme: "dark".into(),
            lang: "zh".into(),
            directory: None,
            prior: Map::new(),
            scope: String::new(),
        }
    }
}

fn legacy() -> Result<Option<Map<String, Value>>, String> {
    let mut size = 0;
    let path = w!(r"Software\UAC2-Control");
    let key = w!("Preferences");
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            path,
            key,
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut size),
        )
    };
    if matches!(result, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
        return Ok(None);
    }
    result.ok().map_err(|e| e.to_string())?;
    if size == 0 || size > 65536 {
        return Err("旧偏好大小异常，已保留注册表原件。".into());
    }
    let mut bytes = vec![0u8; size as usize];
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            path,
            key,
            RRF_RT_REG_BINARY,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes[..size as usize])
        .map(Some)
        .map_err(|e| format!("旧偏好损坏：{e}"))
}
fn accounts(doc: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, String> {
    doc.entry("accounts")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("账户偏好表已损坏。".into())
}
fn entry(
    doc: Option<&Map<String, Value>>,
    key: &str,
) -> Result<Option<Map<String, Value>>, String> {
    let Some(all) = doc.and_then(|x| x.get("accounts")) else {
        return Ok(None);
    };
    let all = all.as_object().ok_or("账户偏好表已损坏。")?;
    all.get(key)
        .map(|x| x.as_object().cloned().ok_or("账户偏好已损坏。".into()))
        .transpose()
}
impl Preferences {
    fn from_record(key: &str, obj: Map<String, Value>) -> Result<Self, String> {
        let mut p: Self = serde_json::from_value(Value::Object(obj)).map_err(|e| e.to_string())?;
        p.prior = p.fields()?;
        p.scope = key.to_owned();
        Ok(p)
    }
    fn fields(&self) -> Result<Map<String, Value>, String> {
        serde_json::to_value(self)
            .map_err(|e| e.to_string())?
            .as_object()
            .cloned()
            .ok_or("偏好无法序列化。".into())
    }
    fn open(migrate: bool) -> Result<Self, String> {
        let (key, own_desktop) = data::account_scope()?;
        let doc = data::read(Document::Settings)?;
        if let Some(obj) = entry(doc.as_ref(), &key)? {
            return Self::from_record(&key, obj);
        }
        let appearance = Self::legacy_appearance(own_desktop, legacy);
        if !migrate || appearance.is_empty() {
            return Self::from_record(&key, appearance);
        }
        let mut actual = None;
        data::update(Document::Settings, |doc| {
            let all = accounts(doc)?;
            if all.len() >= 256 && !all.contains_key(&key) {
                return Err("账户记录已达上限，已有偏好均已保留。".into());
            }
            actual = all
                .entry(key.clone())
                .or_insert(Value::Object(appearance))
                .as_object()
                .cloned();
            actual
                .as_ref()
                .ok_or("账户偏好已损坏。")
                .map(|_| ())
                .map_err(String::from)
        })?;
        Self::from_record(&key, actual.ok_or("迁移后的偏好无法读回。")?)
    }
    fn legacy_appearance(
        own_desktop: bool,
        read: impl FnOnce() -> Result<Option<Map<String, Value>>, String>,
    ) -> Map<String, Value> {
        let mut appearance = Map::new();
        // Old HKCU preferences are shared across copies of the executable.
        // Consent and the library directory belong to this portable account record.
        if own_desktop && let Ok(Some(old)) = read() {
            if let Some(lang @ ("en" | "zh")) = old.get("lang").and_then(Value::as_str) {
                appearance.insert("lang".into(), Value::from(lang));
            }
            if let Some(theme @ ("light" | "dark")) = old.get("theme").and_then(Value::as_str) {
                appearance.insert("theme".into(), Value::from(theme));
            }
        }
        appearance
    }
    pub fn load() -> Result<Self, String> {
        Self::open(false)
    }
    pub fn load_for_desktop() -> Result<Self, String> {
        Self::open(true)
    }
    pub fn save(&mut self) -> Result<(), String> {
        let (key, _) = data::account_scope()?;
        if self.scope != key {
            return Err("当前桌面账户发生变化，请重新打开窗口后再保存。".into());
        }
        let mut merged = None;
        data::update(Document::Settings, |doc| {
            merged = Some(self.merge(doc, &key)?);
            Ok(())
        })?;
        *self = Self::from_record(&key, merged.ok_or("无法确认保存的偏好。")?)?;
        Ok(())
    }
    fn merge(&self, doc: &mut Map<String, Value>, key: &str) -> Result<Map<String, Value>, String> {
        let fields = self.fields()?;
        let all = accounts(doc)?;
        if !all.contains_key(key) && all.len() >= 256 {
            return Err("账户记录已达上限，已有偏好均已保留。".into());
        }
        let has_existing = all.contains_key(key);
        let rec = all
            .entry(key)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or("账户偏好已损坏。")?;
        for (name, value) in fields {
            if !has_existing || self.prior.get(&name) != Some(&value) {
                rec.insert(name, value);
            }
        }
        Ok(rec.clone())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn a_new_portable_account_imports_only_known_appearance() {
        let registry = json!({"accepted":true,"directory":r"C:\old-game","theme":"light","lang":"en","future":2});
        let appearance = Preferences::legacy_appearance(true, || Ok(registry.as_object().cloned()));
        assert_eq!(
            Value::Object(appearance.clone()),
            json!({"theme":"light","lang":"en"})
        );
        let initial = Preferences::from_record("new account", appearance).unwrap();
        assert!(!initial.accepted);
        assert_eq!(initial.directory, None);
        assert_eq!(
            (initial.theme.as_str(), initial.lang.as_str()),
            ("light", "en")
        );
    }

    #[test]
    fn unreadable_or_invalid_legacy_visuals_do_not_block_first_consent() {
        for appearance in [
            Preferences::legacy_appearance(true, || Err("unreadable".into())),
            Preferences::legacy_appearance(true, || Ok(None)),
            Preferences::legacy_appearance(true, || {
                Ok(json!({"lang":3,"theme":"unknown","accepted":true})
                    .as_object()
                    .cloned())
            }),
            Preferences::legacy_appearance(false, || {
                panic!("another account's HKCU must not be read")
            }),
        ] {
            assert!(appearance.is_empty());
            let mut new = Preferences::from_record("current", appearance).unwrap();
            assert!(!new.accepted && new.directory.is_none());
            assert_eq!((new.theme.as_str(), new.lang.as_str()), ("dark", "zh"));
            new.accepted = true;
            let mut doc = Map::new();
            let saved = new.merge(&mut doc, "current").unwrap();
            assert_eq!(saved["accepted"], true);
            assert!(saved["directory"].is_null());
        }
    }

    #[test]
    fn valid_020_local_consent_and_directory_remain_account_specific() {
        let doc = json!({"schema":1,"accounts":{"existing":{"accepted":true,"lang":"zh","theme":"dark","directory":r"C:\chosen-game"}}});
        let existing = entry(doc.as_object(), "existing").unwrap().unwrap();
        let loaded = Preferences::from_record("existing", existing).unwrap();
        assert!(loaded.accepted);
        assert_eq!(loaded.directory, Some(PathBuf::from(r"C:\chosen-game")));
        assert!(entry(doc.as_object(), "different").unwrap().is_none());
    }
    #[test]
    fn account_isolation_stale_edits_and_unknown_preferences() {
        let mut doc = json!({"schema":1,"accounts":{"desktop-A":{"accepted":true,"lang":"en","future":{"keep":3}}}});
        let doc = doc.as_object_mut().unwrap();
        let mut theme =
            Preferences::from_record("desktop-A", entry(Some(doc), "desktop-A").unwrap().unwrap())
                .unwrap();
        let mut language = theme.clone();
        let fresh = Preferences::from_record("desktop-B", Map::new()).unwrap();
        assert!(!fresh.accepted && fresh.directory.is_none());
        language.lang = "zh".into();
        language.merge(doc, "desktop-A").unwrap();
        theme.theme = "light".into();
        let merged = theme.merge(doc, "desktop-A").unwrap();
        assert_eq!(merged["lang"], "zh");
        assert_eq!(merged["future"]["keep"], 3);
        assert!(entry(Some(doc), "desktop-B").unwrap().is_none());
    }
}
