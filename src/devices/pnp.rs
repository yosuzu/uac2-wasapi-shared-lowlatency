use windows::Win32::Devices::{DeviceAndDriverInstallation::*, Properties::*};
use windows::Win32::Foundation::DEVPROPKEY;
use windows::core::{GUID, HSTRING};
fn check(code: CONFIGRET, label: &str) -> Result<(), String> {
    if code == CR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "{label}失败（Configuration Manager 0x{:02X}）。",
            code.0
        ))
    }
}

fn parent(node: u32) -> Result<u32, String> {
    let mut p = 0;
    check(unsafe { CM_Get_Parent(&mut p, node, 0) }, "核对父设备")?;
    Ok(p)
}

pub(super) fn identify(out: &mut super::Endpoint) -> Result<(), String> {
    let ep = locate(&format!(r"SWD\MMDEVAPI\{}", out.id))?;
    let f = parent(ep)?;
    let function = node_id(f)?;
    let service = text_property(f, &DEVPKEY_Device_Service).unwrap_or_default();
    let provider = text_property(f, &DEVPKEY_Device_DriverProvider).unwrap_or_default();
    let inf = text_property(f, &DEVPKEY_Device_DriverInfPath).unwrap_or_default();
    out.driver_service.clone_from(&service);
    out.driver_provider.clone_from(&provider);
    out.driver_inf = inf.rsplit(['\\', '/']).next().unwrap_or_default().to_owned();
    if !service.eq_ignore_ascii_case("usbaudio2")
        || !provider.eq_ignore_ascii_case("Microsoft")
        || !inf.eq_ignore_ascii_case("usbaudio2.inf")
    {
        // An active render endpoint can still be measured with WASAPI. Keep
        // any available connection identity so a reconnection invalidates a run.
        out.arrival = property(f, &DEVPKEY_Device_LastArrivalDate, DEVPROP_TYPE_FILETIME)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .map(u64::from_le_bytes)
            .unwrap_or(0);
        out.function_id = function;
        out.driver_version = text_property(f, &DEVPKEY_Device_DriverVersion).unwrap_or_default();
        return Err(format!(
            "此驱动不支持修改周期：{} {}。修改需要 Microsoft USB Audio 2.0。",
            provider, service
        ));
    }
    let function_upper = function.to_ascii_uppercase();
    let Some(hardware) = function_upper
        .strip_prefix("USB\\")
        .and_then(|s| s.split('\\').next())
    else {
        return Err("无法确认 USB 音频功能。".into());
    };
    if !hardware.starts_with("VID_") || !hardware.contains("&PID_") {
        return Err("无法确认 USB 声卡身份。".into());
    }
    let phys = if hardware.contains("&MI_") {
        parent(f)?
    } else {
        f
    };
    let physical = node_id(phys)?;
    let mut parts = physical.split('\\');
    let valid_parent = parts.next().is_some_and(|s| s.eq_ignore_ascii_case("USB"))
        && parts
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case(hardware.split("&MI_").next().unwrap()))
        && parts.next().is_some_and(|s| !s.is_empty())
        && parts.next().is_none();
    if !valid_parent {
        return Err("USB 物理设备与功能身份不匹配。".into());
    }
    let container = container_id(f)?;
    if container_id(ep)? != container || container_id(phys)? != container {
        return Err("无法核对播放端点的 USB 容器。".into());
    }
    started(ep)?;
    started(f)?;
    started(phys)?;
    out.arrival = u64::from_le_bytes(
        property(f, &DEVPKEY_Device_LastArrivalDate, DEVPROP_TYPE_FILETIME)?
            .try_into()
            .map_err(|_| "无效的设备到达时间")?,
    );
    if out.arrival == 0 {
        return Err("无法确认本次连接。".into());
    }
    out.driver_version = text_property(f, &DEVPKEY_Device_DriverVersion)?;
    out.function_id = function;
    out.physical_id = physical;
    out.container = container;
    let self_id = format!(r"SWD\MMDEVAPI\{}", out.id);
    for (node, id) in children(f, r"SWD\MMDEVAPI\")? {
        if !id.eq_ignore_ascii_case(&self_id) {
            out.sibling_ids.push(id.to_ascii_uppercase());
            out.siblings
                .push(text_property(node, &DEVPKEY_Device_FriendlyName).unwrap_or(id));
        }
    }
    out.siblings.sort();
    out.sibling_ids.sort();
    Ok(())
}
fn node_id(node: u32) -> Result<String, String> {
    let mut buf = [0; MAX_DEVICE_ID_LEN as usize];
    check(
        unsafe { CM_Get_Device_IDW(node, &mut buf, 0) },
        "读取 PnP 实例 ID",
    )?;
    let end = buf
        .iter()
        .position(|&c| c == 0)
        .ok_or("PnP 实例 ID 被截断。")?;
    String::from_utf16(&buf[..end]).map_err(|_| "PnP 实例 ID 无效。".into())
}

fn locate(id: &str) -> Result<u32, String> {
    let mut node = 0;
    check(
        unsafe { CM_Locate_DevNodeW(&mut node, &HSTRING::from(id), CM_LOCATE_DEVNODE_NORMAL) },
        "定位在场设备",
    )?;
    Ok(node)
}

fn children(parent: u32, prefix: &str) -> Result<Vec<(u32, String)>, String> {
    let mut result = Vec::new();
    let mut node = 0;
    let mut status = unsafe { CM_Get_Child(&mut node, parent, 0) };
    while status != CR_NO_SUCH_DEVNODE {
        check(status, "枚举直接子设备")?;
        let id = node_id(node)?;
        if id.to_ascii_uppercase().starts_with(prefix) {
            let mut actual_parent = 0;
            check(
                unsafe { CM_Get_Parent(&mut actual_parent, node, 0) },
                "核对子设备的父节点",
            )?;
            if actual_parent != parent {
                return Err("枚举期间设备的父子关系已改变。".into());
            }
            result.push((node, id));
        }
        let current = node;
        status = unsafe { CM_Get_Sibling(&mut node, current, 0) };
    }
    Ok(result)
}

fn read_property(
    expected: DEVPROPTYPE,
    query: impl Fn(&mut DEVPROPTYPE, Option<*mut u8>, &mut u32) -> CONFIGRET,
) -> Result<Vec<u8>, String> {
    let mut kind = DEVPROPTYPE::default();
    let mut len = 0;
    let size_result = query(&mut kind, None, &mut len);
    if size_result != CR_BUFFER_SMALL {
        check(size_result, "查询 PnP 属性大小")?;
    }
    if !(1..=65536).contains(&len) {
        return Err("PnP 属性长度无效。".into());
    }
    let mut data = vec![0; len as usize];
    check(
        query(&mut kind, Some(data.as_mut_ptr()), &mut len),
        "读取 PnP 属性",
    )?;
    if kind != expected || len as usize > data.len() {
        return Err("PnP 属性的类型或长度不符。".into());
    }
    data.truncate(len as usize);
    Ok(data)
}

fn property(node: u32, key: &DEVPROPKEY, expected: DEVPROPTYPE) -> Result<Vec<u8>, String> {
    read_property(expected, |kind, buf, len| unsafe {
        CM_Get_DevNode_PropertyW(node, key, kind, buf, len, 0)
    })
}

fn decode_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() % 2 != 0 {
        return Err("PnP 文本属性被截断。".into());
    }
    let mut units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    if units.pop() != Some(0) || units.contains(&0) {
        return Err("PnP 文本属性格式无效。".into());
    }
    String::from_utf16(&units).map_err(|_| "PnP 文本属性编码无效。".into())
}

fn text_property(node: u32, key: &DEVPROPKEY) -> Result<String, String> {
    decode_text(&property(node, key, DEVPROP_TYPE_STRING)?)
}

fn container_id(node: u32) -> Result<String, String> {
    let bytes = property(node, &DEVPKEY_Device_ContainerId, DEVPROP_TYPE_GUID)?;
    if bytes.len() != size_of::<GUID>() || bytes.iter().all(|b| *b == 0) {
        return Err("设备容器 ID 缺失或无效。".into());
    }
    let guid = unsafe { bytes.as_ptr().cast::<GUID>().read_unaligned() };
    Ok(format!("{guid:?}").to_ascii_lowercase())
}

fn started(node: u32) -> Result<(), String> {
    let mut flags = CM_DEVNODE_STATUS_FLAGS::default();
    let mut problem = CM_PROB::default();
    check(
        unsafe { CM_Get_DevNode_Status(&mut flags, &mut problem, node, 0) },
        "检查设备状态",
    )?;
    if !flags.contains(DN_STARTED) || flags.contains(DN_HAS_PROBLEM) || problem.0 != 0 {
        return Err(format!("设备尚未正常启动（设备问题代码 {}）。", problem.0));
    }
    Ok(())
}
