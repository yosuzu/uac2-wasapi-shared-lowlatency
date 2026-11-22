//! Local portable storage. The elevated writer accepts fixed names only.
//! Windows checks the owner and protected ACL of opened handles. Nothing in
//! the WebView profile acts as authority for a device operation.
use serde_json::{Map, Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
};
use windows::{
    Win32::{
        Foundation::{
            ERROR_ALREADY_EXISTS, HANDLE, HLOCAL, LocalFree, WAIT_ABANDONED, WAIT_OBJECT_0,
        },
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{Com::CoCreateGuid, Registry::*, Threading::*},
    },
    core::{HSTRING, Owned, PWSTR, w},
};

type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
const PRIVATE: &str = "O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)";
const REPORT: &str = "O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;BU)";

struct Local(*mut std::ffi::c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0)));
        }
    }
}
struct Policy {
    memory: Local,
    expected: String,
}
impl Policy {
    fn new(sddl: &str) -> Result<Self> {
        let mut d = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                1,
                &mut d,
                None,
            )
        }
        .map_err(err)?;
        let memory = Local(d.0);
        Ok(Self {
            memory,
            expected: Self::describe(d)?,
        })
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.memory.0,
            bInheritHandle: false.into(),
        }
    }
    fn describe(d: PSECURITY_DESCRIPTOR) -> Result<String> {
        let mut text = PWSTR::null();
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                d,
                1,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
        }
        .map_err(err)?;
        let _mem = Local(text.0.cast());
        unsafe { text.to_string() }.map_err(err)
    }
    fn check(&self, file: &File) -> Result<()> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe {
            GetSecurityInfo(
                HANDLE(file.as_raw_handle()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&mut sd),
            )
        }
        .ok()
        .map_err(err)?;
        let _mem = Local(sd.0);
        if Self::describe(sd)? != self.expected {
            return Err("数据目录或文件的所有者或写入权限已改变。请保留原件，在本机 NTFS 目录中恢复管理员权限。".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub enum Document {
    Settings,
    Recovery,
}
impl Document {
    fn name(self) -> &'static str {
        match self {
            Self::Settings => "settings.json",
            Self::Recovery => "device-recovery.json",
        }
    }
    fn limit(self) -> usize {
        match self {
            Self::Settings => 256 * 1024,
            Self::Recovery => 32 * 1024 * 1024,
        }
    }
}

struct Directory {
    path: PathBuf,
    _pins: Vec<File>,
    policy: Policy,
}
impl Directory {
    fn open(parent: &Path, diagnostics: bool, create: bool) -> Result<Option<Self>> {
        let policy = Policy::new(if diagnostics { REPORT } else { PRIVATE })?;
        Self::open_with_policy(parent, diagnostics, create, policy)
    }
    fn open_with_policy(
        parent: &Path,
        diagnostics: bool,
        create: bool,
        policy: Policy,
    ) -> Result<Option<Self>> {
        let mut path = PathBuf::new();
        let mut pins = Vec::new();
        let root = parent.join("uac2-data");
        let dest = root.join(if diagnostics {
            "diagnostics"
        } else {
            "private"
        });
        for component in dest.components() {
            path.push(component.as_os_str());
            match component {
                Component::Prefix(p)
                    if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) =>
                {
                    continue;
                }
                Component::RootDir => {
                    if !matches!(
                        unsafe { GetDriveTypeW(&HSTRING::from(path.as_os_str())) },
                        2 | 3 | 6
                    ) {
                        return Err("请将工具放在本地磁盘上。".into());
                    }
                    let mut flags = 0;
                    unsafe {
                        GetVolumeInformationW(
                            &HSTRING::from(path.as_os_str()),
                            None,
                            None,
                            None,
                            Some(&mut flags),
                            None,
                        )
                    }
                    .map_err(|e| format!("无法验证本地卷的安全权限：{e}"))?;
                    if flags & 8 == 0 {
                        return Err("此磁盘不能持久保存 Windows 访问权限。请将程序与 uac2-data 放到本地 NTFS 文件夹。".into());
                    }
                    continue;
                }
                Component::Normal(_) => {}
                _ => return Err("本地数据需要本机磁盘上的绝对路径。".into()),
            }
            if create && (path == root || path == dest) {
                let sa = policy.attributes();
                if let Err(e) = unsafe {
                    CreateDirectoryW(
                        &HSTRING::from(path.as_os_str()),
                        (path == dest).then_some(&sa as *const _),
                    )
                } {
                    if e.code() != windows::core::HRESULT::from_win32(ERROR_ALREADY_EXISTS.0) {
                        return Err(format!(
                            "无法建立安全数据目录 {}。请使用可写的本地 NTFS 文件夹：{e}",
                            path.display()
                        ));
                    }
                }
            }
            let f = match OpenOptions::new()
                .access_mode(0x80 | if path == dest { 0x20000 } else { 0 })
                .share_mode(1)
                .custom_flags((FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).0)
                .open(&path)
            {
                Err(e) if !create && e.kind() == ErrorKind::NotFound => return Ok(None),
                r => r.map_err(|e| format!("无法核对数据目录 {}：{e}", path.display()))?,
            };
            let m = f.metadata().map_err(err)?;
            if !m.is_dir() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                return Err("数据路径含重解析点或非常规目录。".into());
            }
            if path == dest {
                policy.check(&f)?;
            }
            pins.push(f);
        }
        Ok(Some(Self {
            path: dest,
            _pins: pins,
            policy,
        }))
    }
    fn file(&self, name: &str) -> Result<Option<File>> {
        let f = match OpenOptions::new()
            .read(true)
            .share_mode(1 | 4)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(self.path.join(name))
        {
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            x => x.map_err(err)?,
        };
        self.check_file(&f)?;
        Ok(Some(f))
    }
    fn check_file(&self, f: &File) -> Result<()> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(f.as_raw_handle()), &mut info) }.map_err(err)?;
        if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT).0 != 0
            || info.nNumberOfLinks != 1
        {
            return Err("数据文件存在重解析点、硬链接或类型错误。".into());
        }
        self.policy.check(f)
    }
    fn read(&self, name: &str, limit: usize) -> Result<Option<Vec<u8>>> {
        let Some(file) = self.file(name)? else {
            return Ok(None);
        };
        if file.metadata().map_err(err)?.len() > limit as u64 {
            return Err("本地数据超过大小限制。".into());
        }
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() > limit {
            return Err("本地数据超过大小限制。".into());
        }
        Ok(Some(bytes))
    }
    fn replace(&self, name: &str, bytes: &[u8], limit: usize) -> Result<()> {
        if bytes.len() > limit {
            return Err("本地数据超过大小限制，已保留原件。".into());
        }
        drop(self.file(name)?);
        // Fixed staging name bounds interrupted writes. The directory's DACL
        // excludes unprivileged mutation; never open an existing file for writing.
        let temp = format!("{name}.tmp");
        if self.file(&temp)?.is_some() {
            fs::remove_file(self.path.join(&temp)).map_err(err)?;
        }
        let from = self.path.join(&temp);
        let to = self.path.join(name);
        let sa = self.policy.attributes();
        let handle = unsafe {
            CreateFileW(
                &HSTRING::from(from.as_os_str()),
                0xC0000000,
                FILE_SHARE_READ,
                Some(&sa),
                CREATE_NEW,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH,
                None,
            )
        }
        .map_err(|e| {
            format!(
                "无法建立 {} 的受保护临时文件。请核对程序目录写入权限：{e}",
                to.display()
            )
        })?;
        let mut f = unsafe { File::from_raw_handle(handle.0) };
        self.check_file(&f)?;
        f.write_all(bytes).map_err(err)?;
        f.sync_all().map_err(err)?;
        drop(f);
        unsafe {
            MoveFileExW(
                &HSTRING::from(from.as_os_str()),
                &HSTRING::from(to.as_os_str()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(err)?;
        if self.read(name, limit)?.as_deref() != Some(bytes) {
            return Err("本地数据持久化后读回不一致。".into());
        }
        Ok(())
    }
}

struct WriteLock(Owned<HANDLE>);
impl WriteLock {
    fn get() -> Result<Self> {
        let h = unsafe {
            Owned::new(
                CreateMutexW(None, false, w!(r"Global\UAC2.Control.PortableData.v1"))
                    .map_err(err)?,
            )
        };
        match unsafe { WaitForSingleObject(*h, 5000) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(h)),
            _ => Err("另一项本地数据更新尚未完成，请重试。".into()),
        }
    }
}
impl Drop for WriteLock {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(*self.0);
        }
    }
}

fn parent() -> Result<PathBuf> {
    std::env::current_exe()
        .map_err(err)?
        .parent()
        .map(Path::to_owned)
        .ok_or("无法定位程序目录。".into())
}
fn parse(doc: Document, bytes: &[u8]) -> Result<Map<String, Value>> {
    let obj: Map<String, Value> =
        serde_json::from_slice(bytes).map_err(|e| format!("本地数据文件损坏：{e}"))?;
    if obj.get("schema").and_then(Value::as_u64) != Some(1) {
        return Err("未知的本地数据文件版本。".into());
    }
    let table = match doc {
        Document::Recovery => "endpoints",
        Document::Settings => "accounts",
    };
    if !obj
        .get(table)
        .is_some_and(|t| t.as_object().is_some_and(|m| m.len() <= 256))
    {
        return Err("本地数据表缺失、超出容量或格式损坏。".into());
    }
    if matches!(doc, Document::Recovery)
        && !obj
            .get("revision")
            .is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty()))
    {
        return Err("恢复文件修订号缺失。".into());
    }
    Ok(obj)
}
pub(crate) fn digest(bytes: &[u8]) -> Result<String> {
    use windows::Win32::Security::Cryptography::CryptHashCertificate2;
    let mut hash = [0u8; 32];
    let mut size = 32;
    unsafe {
        CryptHashCertificate2(
            w!("SHA256"),
            0,
            None,
            Some(bytes),
            Some(hash.as_mut_ptr()),
            &mut size,
        )
    }
    .map_err(err)?;
    if size != 32 {
        return Err("数据摘要长度异常。".into());
    }
    Ok(hash.iter().map(|b| format!("{b:02x}")).collect())
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Seal {
    committed: Option<String>,
    pending: Option<String>,
}
impl Seal {
    fn check(&self, bytes: Option<&[u8]>) -> Result<()> {
        if (self.committed.is_none() && self.pending.is_none())
            || self
                .committed
                .iter()
                .chain(&self.pending)
                .any(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("恢复文件本地校验记录含无效摘要。".into());
        }
        let sha = bytes.map(digest).transpose()?;
        if sha == self.committed || (sha.is_some() && sha == self.pending) {
            return Ok(());
        }
        Err("device-recovery.json 缺失、损坏或已退回旧版。请从同一份备份恢复整个 private 目录，再操作设备。".into())
    }
}
const SEAL: &str = "recovery.integrity.json";
fn load_bytes(dir: &Directory, doc: Document) -> Result<Option<Vec<u8>>> {
    if !matches!(doc, Document::Recovery) {
        return dir.read(doc.name(), doc.limit());
    }
    for _ in 0..3 {
        let first = dir.read(SEAL, 1024)?;
        let bytes = dir.read(doc.name(), doc.limit())?;
        if first != dir.read(SEAL, 1024)? {
            continue;
        }
        match first {
            Some(seal) => serde_json::from_slice::<Seal>(&seal)
                .map_err(|_| "恢复文件本地校验记录损坏。")?
                .check(bytes.as_deref())?,
            None if bytes.is_some() => {
                return Err(
                    "设备恢复文件缺少配套的本地校验记录，请从同一备份恢复 private 目录。".into(),
                );
            }
            None => {}
        }
        return Ok(bytes);
    }
    Err("另一项恢复文件更新尚未完成，请重试。".into())
}
fn load(dir: &Directory, doc: Document) -> Result<Option<Map<String, Value>>> {
    load_bytes(dir, doc)?
        .as_deref()
        .map(|b| parse(doc, b))
        .transpose()
}
pub fn read(doc: Document) -> Result<Option<Map<String, Value>>> {
    let Some(dir) = Directory::open(&parent()?, false, false)? else {
        return Ok(None);
    };
    load(&dir, doc)
}
pub fn update(doc: Document, f: impl FnOnce(&mut Map<String, Value>) -> Result<()>) -> Result<()> {
    let _lock = WriteLock::get()?;
    let dir = Directory::open(&parent()?, false, true)?.ok_or("未找到本地数据目录。")?;
    update_in(&dir, doc, f)
}
fn update_in(
    dir: &Directory,
    doc: Document,
    f: impl FnOnce(&mut Map<String, Value>) -> Result<()>,
) -> Result<()> {
    let before = load_bytes(dir, doc)?;
    let mut obj = before
        .as_deref()
        .map(|b| parse(doc, b))
        .transpose()?
        .unwrap_or_else(|| Map::from_iter([("schema".into(), json!(1))]));
    f(&mut obj)?;
    let bytes = serde_json::to_vec(&obj).map_err(err)?;
    if bytes.len() > doc.limit() {
        return Err("本地数据超过大小限制，已保留原件。".into());
    }
    parse(doc, &bytes)?;
    if matches!(doc, Document::Recovery) {
        // Persist the two permitted digests before replacing the document.
        // At every crash boundary either the preimage or new image verifies.
        let next = digest(&bytes)?;
        let transaction = Seal {
            committed: before.as_deref().map(digest).transpose()?,
            pending: Some(next.clone()),
        };
        dir.replace(SEAL, &serde_json::to_vec(&transaction).map_err(err)?, 1024)?;
        dir.replace(doc.name(), &bytes, doc.limit())?;
        let completed = Seal {
            committed: Some(next),
            pending: None,
        };
        dir.replace(SEAL, &serde_json::to_vec(&completed).map_err(err)?, 1024)?;
    } else {
        dir.replace(doc.name(), &bytes, doc.limit())?;
    }
    Ok(())
}
pub fn write_diagnostic(bytes: &[u8]) -> Result<PathBuf> {
    let _lock = WriteLock::get()?;
    let dir = Directory::open(&parent()?, true, true)?.ok_or("无法打开诊断目录。")?;
    dir.replace("latest.json", bytes, 2 * 1024 * 1024)?;
    Ok(dir.path.join("latest.json"))
}
fn account_for(process: HANDLE) -> Result<String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(err)?;
    let token = unsafe { Owned::new(token) };
    // TokenUser contains the SID followed by no variable data beyond SECURITY_MAX_SID_SIZE.
    let mut buf = [0usize; 32];
    let mut used = 0;
    unsafe {
        GetTokenInformation(
            *token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            size_of_val(&buf) as u32,
            &mut used,
        )
    }
    .map_err(err)?;
    let user = unsafe { &*buf.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) }.map_err(err)?;
    let _mem = Local(sid.0.cast());
    unsafe { sid.to_string() }.map_err(err)
}
pub fn account_scope() -> Result<(String, bool)> {
    use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};
    let admin = account_for(unsafe { GetCurrentProcess() })?;
    let shell = unsafe { GetShellWindow() };
    let mut pid = 0;
    if shell.0.is_null()
        || unsafe { GetWindowThreadProcessId(shell, Some(&mut pid)) } == 0
        || pid == 0
    {
        return Err("无法识别当前 Windows 桌面账户，无法读取首次同意与路径偏好。".into());
    }
    let process = unsafe {
        Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(err)?)
    };
    let desktop = account_for(*process)?;
    Ok((
        format!("{}/{admin}/{desktop}", machine()?),
        admin == desktop,
    ))
}
pub fn machine() -> Result<String> {
    let mut buf = [0u16; 64];
    let mut size = size_of_val(&buf) as u32;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Cryptography"),
            w!("MachineGuid"),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .map_err(err)?;
    let id = String::from_utf16(
        &buf[..buf
            .iter()
            .position(|&c| c == 0)
            .ok_or("机器标识格式无效。")?],
    )
    .map_err(err)?;
    if id.len() != 36
        || !id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err("机器标识格式无效。".into());
    }
    Ok(id.to_ascii_lowercase())
}
pub fn revision() -> Result<String> {
    unsafe { CoCreateGuid() }
        .map(|id| format!("{id:?}"))
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seal_accepts_only_the_two_crash_images_and_detects_individual_rollback() {
        let a = digest(b"old").unwrap();
        let b = digest(b"new").unwrap();
        let transition = Seal {
            committed: Some(a.clone()),
            pending: Some(b.clone()),
        };
        assert!(transition.check(Some(b"old")).is_ok());
        assert!(transition.check(Some(b"new")).is_ok());
        assert!(transition.check(None).is_err());
        assert!(transition.check(Some(b"other")).is_err());
        let complete = Seal {
            committed: Some(b.clone()),
            pending: None,
        };
        assert!(complete.check(Some(b"old")).is_err());
        assert!(complete.check(None).is_err());
        assert!(
            Seal {
                committed: None,
                pending: None
            }
            .check(None)
            .is_err()
        );
        assert!(
            Seal {
                committed: None,
                pending: Some(b)
            }
            .check(None)
            .is_ok()
        );
    }
    #[test]
    fn future_or_incomplete_documents_refuse_updates() {
        assert!(
            parse(
                Document::Recovery,
                br#"{"schema":2,"endpoints":{},"revision":"future"}"#
            )
            .is_err()
        );
        assert!(parse(Document::Recovery, br#"{"schema":1,"revision":"a"}"#).is_err());
        assert!(parse(Document::Settings, br#"{"schema":1,"accounts":null}"#).is_err());
    }
    #[test]
    fn isolated_ntfs_atomic_replace_link_policy_and_lost_recovery() {
        let temp = parent().unwrap().join(format!("uac2-storage-test-{}", revision().unwrap()));
        fs::create_dir(&temp).unwrap();
        // Run filesystem verification as an ordinary user. The test-only ACL
        // uses the test process SID; production constructors require BA.
        let sid = account_for(unsafe { GetCurrentProcess() }).unwrap();
        let rule = format!("O:{sid}D:P(A;;FA;;;{sid})");
        let dir = Directory::open_with_policy(&temp, false, true, Policy::new(&rule).unwrap())
            .unwrap()
            .unwrap();
        assert!(
            Policy::new(PRIVATE)
                .unwrap()
                .check(dir._pins.last().unwrap())
                .is_err()
        );
        let update = |version: &str| {
            update_in(&dir, Document::Recovery, |obj| {
                obj.insert(
                    "endpoints".into(),
                    json!({"fake endpoint":{"test":"no device access"}}),
                );
                obj.insert("revision".into(), json!(version));
                Ok(())
            })
        };
        update("one").unwrap();
        let old = dir
            .read(Document::Recovery.name(), Document::Recovery.limit())
            .unwrap()
            .unwrap();
        update("two").unwrap();
        let current = dir
            .read(Document::Recovery.name(), Document::Recovery.limit())
            .unwrap()
            .unwrap();
        assert_eq!(
            load(&dir, Document::Recovery).unwrap().unwrap()["revision"],
            "two"
        );
        dir.replace(Document::Recovery.name(), &old, Document::Recovery.limit())
            .unwrap();
        assert!(load(&dir, Document::Recovery).is_err());
        assert!(update("three").is_err());
        dir.replace(
            Document::Recovery.name(),
            &current,
            Document::Recovery.limit(),
        )
        .unwrap();
        let target = dir.path.join(Document::Recovery.name());
        let link = dir.path.join("other.json");
        fs::hard_link(&target, &link).unwrap();
        assert!(
            dir.read(Document::Recovery.name(), Document::Recovery.limit())
                .is_err()
        );
        assert!(update("three").is_err());
        fs::remove_file(&link).unwrap();
        fs::remove_file(&target).unwrap();
        assert!(update("three").is_err());
        assert!(!target.exists());
        fs::remove_file(dir.path.join(SEAL)).unwrap();
        let private = dir.path.clone();
        drop(dir);
        fs::remove_dir(private).unwrap();
        fs::remove_dir(temp.join("uac2-data")).unwrap();
        fs::remove_dir(temp).unwrap();
    }
}
