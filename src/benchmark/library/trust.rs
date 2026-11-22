use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows::Win32::{
    Foundation::{HANDLE, HWND},
    Security::{Cryptography::*, WinTrust::*},
};
use windows::core::{PCSTR, PCWSTR};

fn subject(cert: *const CERT_CONTEXT, oid: PCSTR) -> Option<String> {
    let attr = Some(oid.0.cast());
    let length = unsafe { CertGetNameStringW(cert, CERT_NAME_ATTR_TYPE, 0, attr, None) };
    if !(2..=2048).contains(&length) {
        return None;
    }
    let mut name = vec![0; length as usize];
    let received =
        unsafe { CertGetNameStringW(cert, CERT_NAME_ATTR_TYPE, 0, attr, Some(&mut name)) };
    if received != length {
        return None;
    }
    String::from_utf16(&name[..(length - 1) as usize]).ok()
}

// Verify the embedded Authenticode signature of the same opened image that is locked
// against writes/deletion. Consult local Windows trust policy and cached material only.
pub(super) fn publisher(file: &File, path: &Path) -> Result<(), String> {
    let wide = super::wide(path.as_os_str());
    let mut image = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        hFile: HANDLE(file.as_raw_handle()),
        ..Default::default()
    };
    let mut state = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut image },
        dwStateAction: WTD_STATEACTION_VERIFY,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_DISABLE_MD2_MD4,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let no_ui = HWND((-1isize) as _);
    unsafe {
        let status = WinVerifyTrust(no_ui, &mut action, (&raw mut state).cast());
        let result = (|| {
            if status != 0 {
                return Err(format!("Windows 签名验证失败（0x{:08X}）。", status as u32));
            }
            let data = WTHelperProvDataFromStateData(state.hWVTStateData);
            if data.is_null() {
                return Err("无法读取已验证的签名发布者。".into());
            }
            let signer = WTHelperGetProvSignerFromChain(data, 0, false, 0);
            if signer.is_null() {
                return Err("无法读取已验证的签名发布者。".into());
            }
            let leaf = WTHelperGetProvCertFromChain(signer, 0);
            if leaf.is_null() || (*leaf).pCert.is_null() {
                return Err("无法读取已验证的签名发布者。".into());
            }
            let org = subject((*leaf).pCert, szOID_ORGANIZATION_NAME);
            let cn = subject((*leaf).pCert, szOID_COMMON_NAME);
            const VENDOR: &str = "Un4seen Developments Ltd";
            if !org
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(VENDOR))
                || !cn
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(VENDOR))
            {
                return Err("签名发布者不是 Un4seen Developments Ltd。".into());
            }
            Ok(())
        })();
        state.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(no_ui, &mut action, (&raw mut state).cast());
        result
    }
}
