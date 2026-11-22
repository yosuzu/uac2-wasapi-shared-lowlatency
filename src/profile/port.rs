//! Scoped USB port reconnection, adapted from ZH3 Control's verified hub queries.
//! The removal fallback and a separately confirmed restart have different guards.
use super::native;
use crate::devices::Endpoint;
use std::mem::{offset_of, size_of};
use windows::Win32::{
    Devices::{DeviceAndDriverInstallation::*, Properties::*, Usb::*},
    Foundation::{CloseHandle, DEVPROPKEY, GENERIC_WRITE, HANDLE},
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    },
    System::IO::DeviceIoControl,
};
use windows::core::HSTRING;

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortTarget {
    pub port: u32,
    pub usb: String,
    pub ks: String,
    pub container: String,
    pub driver: String,
    pub driver_key: String,
    pub hub_instance: String,
    pub hub_link: String,
    /// A replug into the same port can preserve the instance ID and driver key.
    pub usb_arrival: u64,
}

fn cm(code: CONFIGRET, label: &str) -> Result<()> {
    if code == CR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "{label}失败（Configuration Manager 0x{:02X}）。",
            code.0
        ))
    }
}

fn property(node: u32, key: &DEVPROPKEY, ty: DEVPROPTYPE) -> Result<Vec<u8>> {
    let mut kind = DEVPROPTYPE::default();
    let mut size = 0u32;
    let rc = unsafe { CM_Get_DevNode_PropertyW(node, key, &mut kind, None, &mut size, 0) };
    if rc != CR_BUFFER_SMALL {
        cm(rc, "查询 所选声卡 USB 端口属性大小")?;
    }
    if !(1..=65536).contains(&size) {
        return Err("所选声卡 USB 端口属性长度无效。".into());
    }
    let mut buf = vec![0u8; size as usize];
    cm(
        unsafe {
            CM_Get_DevNode_PropertyW(node, key, &mut kind, Some(buf.as_mut_ptr()), &mut size, 0)
        },
        "读取 所选声卡 USB 端口属性",
    )?;
    if kind != ty || size as usize > buf.len() {
        return Err("所选声卡 USB 端口属性的类型或长度发生变化。".into());
    }
    buf.truncate(size as usize);
    Ok(buf)
}

fn terminated_string(bytes: &[u8]) -> Result<String> {
    if bytes.len() % 2 != 0 {
        return Err("USB 驱动键名被截断。".into());
    }
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .collect();
    let end = units
        .iter()
        .position(|&v| v == 0)
        .ok_or("USB 驱动键名缺少结束符。")?;
    if end == 0 || units[end..].iter().any(|&v| v != 0) {
        return Err("USB 驱动键名为空或包含多余数据。".into());
    }
    String::from_utf16(&units[..end]).map_err(|_| "USB 驱动键名不是有效的 UTF-16。".into())
}

fn hub_link(parent: &str) -> Result<String> {
    let id = HSTRING::from(parent);
    let mut length = 0u32;
    let flags = CM_GET_DEVICE_INTERFACE_LIST_PRESENT;
    cm(
        unsafe {
            CM_Get_Device_Interface_List_SizeW(&mut length, &GUID_DEVINTERFACE_USB_HUB, &id, flags)
        },
        "定位 所选声卡 的父 USB Hub",
    )?;
    if !(1..=65536).contains(&length) {
        return Err("父 USB Hub 接口清单长度无效。".into());
    }
    let mut buf = vec![0u16; length as usize];
    cm(
        unsafe { CM_Get_Device_Interface_ListW(&GUID_DEVINTERFACE_USB_HUB, &id, &mut buf, flags) },
        "读取父 USB Hub 接口",
    )?;
    let mut links = buf.split(|&v| v == 0).take_while(|s| !s.is_empty());
    let link = links
        .next()
        .ok_or("所选声卡 的直接父设备没有在场 USB Hub 接口。")?;
    if links.next().is_some() {
        return Err("所选声卡 的直接父设备有多个 USB Hub 接口。".into());
    }
    String::from_utf16(link).map_err(|_| "父 USB Hub 接口名无效。".into())
}

fn model(id: &str) -> Result<(u16, u16)> {
    let hardware = id
        .split('\\')
        .nth(1)
        .ok_or("缺少 USB 硬件标识。")?
        .to_ascii_uppercase();
    let mut parts = hardware.split('&');
    let vendor = parts
        .next()
        .and_then(|s| s.strip_prefix("VID_"))
        .filter(|s| s.len() == 4);
    let product = parts
        .next()
        .and_then(|s| s.strip_prefix("PID_"))
        .filter(|s| s.len() == 4);
    match (vendor, product, parts.next()) {
        (Some(v), Some(p), None) => Ok((
            u16::from_str_radix(v, 16).map_err(|_| "无效 VID")?,
            u16::from_str_radix(p, 16).map_err(|_| "无效 PID")?,
        )),
        _ => Err("无法唯一解出物理 USB 的 VID/PID。".into()),
    }
}

// A reappearing audio function may carry a different instance suffix. Check
// both the old ID and every direct child with its hardware/interface identity.
fn absent(e: &Endpoint) -> Result<()> {
    if e.physical_id.eq_ignore_ascii_case(&e.function_id) {
        return Err("独立设备实例已卸载，须手动重新插拔。".into());
    }
    let key = e
        .function_id
        .split('\\')
        .nth(1)
        .ok_or("无效音频功能标识。")?;
    let prefix = format!("USB\\{key}\\").to_ascii_uppercase();
    for flags in [CM_LOCATE_DEVNODE_NORMAL, CM_LOCATE_DEVNODE_PHANTOM] {
        let mut node = 0;
        let rc = unsafe { CM_Locate_DevNodeW(&mut node, &HSTRING::from(&e.function_id), flags) };
        if rc != CR_NO_SUCH_DEVNODE {
            return Err("原音频功能尚在场或已返回，跳过端口重连。".into());
        }
    }
    let mut parent = 0;
    cm(
        unsafe {
            CM_Locate_DevNodeW(
                &mut parent,
                &HSTRING::from(&e.physical_id),
                CM_LOCATE_DEVNODE_NORMAL,
            )
        },
        "定位原物理设备",
    )?;
    let mut node = 0;
    let mut rc = unsafe { CM_Get_Child(&mut node, parent, 0) };
    for _ in 0..256 {
        if rc == CR_NO_SUCH_DEVNODE {
            return Ok(());
        }
        cm(rc, "枚举物理设备的功能")?;
        let mut id = [0; MAX_DEVICE_ID_LEN as usize];
        cm(
            unsafe { CM_Get_Device_IDW(node, &mut id, 0) },
            "读取功能实例",
        )?;
        let n = id.iter().position(|x| *x == 0).ok_or("功能实例过长。")?;
        let id = String::from_utf16(&id[..n]).map_err(|_| "无效功能实例 ID")?;
        if id.to_ascii_uppercase().starts_with(&prefix) {
            return Err("音频功能已重新出现，跳过端口重连。".into());
        }
        let prev = node;
        rc = unsafe { CM_Get_Sibling(&mut node, prev, 0) };
    }
    Err("设备的子功能清单无法核对。".into())
}

enum PortLookup {
    Discover,
    Unique,
    Exact(u32),
}

fn attachment(d: &Endpoint, lookup: PortLookup, access: u32) -> Result<(PortTarget, Hub)> {
    let model = model(&d.physical_id)?;
    native::started(&d.physical_id)?;
    let mut node = 0;
    cm(
        unsafe {
            CM_Locate_DevNodeW(
                &mut node,
                &HSTRING::from(&d.physical_id),
                CM_LOCATE_DEVNODE_NORMAL,
            )
        },
        "定位 所选声卡 USB 实例",
    )?;
    let bytes = property(node, &DEVPKEY_Device_ContainerId, DEVPROP_TYPE_GUID)?;
    if bytes.len() != 16 {
        return Err("物理容器 ID 长度异常。".into());
    }
    let container = unsafe {
        bytes
            .as_ptr()
            .cast::<windows::core::GUID>()
            .read_unaligned()
    };
    if !format!("{container:?}").eq_ignore_ascii_case(d.container.trim_matches(['{', '}'])) {
        return Err("物理设备容器已改变。".into());
    }
    let mut parent = 0;
    cm(
        unsafe { CM_Get_Parent(&mut parent, node, 0) },
        "定位 所选声卡 的直接父设备",
    )?;
    let mut id = [0u16; MAX_DEVICE_ID_LEN as usize];
    cm(
        unsafe { CM_Get_Device_IDW(parent, &mut id, 0) },
        "读取父 USB Hub 实例 ID",
    )?;
    let end = id
        .iter()
        .position(|&v| v == 0)
        .ok_or("父 Hub 实例 ID 被截断。")?;
    let hub_instance = String::from_utf16(&id[..end]).map_err(|_| "父 Hub 实例 ID 无效。")?;
    let hub_link = hub_link(&hub_instance)?;
    let usb_arrival = u64::from_le_bytes(
        property(node, &DEVPKEY_Device_LastArrivalDate, DEVPROP_TYPE_FILETIME)?
            .try_into()
            .map_err(|_| "USB 设备到达时间格式错误。")?,
    );
    if usb_arrival == 0 {
        return Err("缺少 所选声卡 本次 USB 连接的到达时间。".into());
    }
    let driver_key = terminated_string(&property(
        node,
        &DEVPKEY_Device_Driver,
        DEVPROP_TYPE_STRING,
    )?)?;
    let hub = Hub::open(&hub_link, access)?;
    let port = match lookup {
        PortLookup::Exact(port) => {
            if !hub.matches(port, &driver_key, model)? {
                return Err("已确认的 USB 端口现在没有连接同一台 所选声卡。".into());
            }
            port
        }
        PortLookup::Unique => find_port(
            None,
            || hub.highest_port(),
            |port| hub.matches(port, &driver_key, model),
        )?,
        PortLookup::Discover => {
            // DEVPKEY_Device_Address is bus-specific. It is only a fast hint,
            // never evidence that the port belongs to this physical instance.
            let hint = property(node, &DEVPKEY_Device_Address, DEVPROP_TYPE_UINT32)
                .ok()
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_le_bytes);
            find_port(
                hint,
                || hub.highest_port(),
                |port| hub.matches(port, &driver_key, model),
            )?
        }
    };
    Ok((
        PortTarget {
            port,
            usb: d.physical_id.to_ascii_uppercase(),
            ks: d.function_id.to_ascii_uppercase(),
            container: d.container.to_ascii_lowercase(),
            driver: d.driver_version.clone(),
            driver_key: driver_key.to_ascii_lowercase(),
            hub_instance: hub_instance.to_ascii_uppercase(),
            hub_link: hub_link.to_ascii_lowercase(),
            usb_arrival,
        },
        hub,
    ))
}

fn find_port(
    hint: Option<u32>,
    highest: impl FnOnce() -> Result<u32>,
    mut matches: impl FnMut(u32) -> Result<bool>,
) -> Result<u32> {
    if let Some(port @ 1..=255) = hint {
        if matches(port).unwrap_or(false) {
            return Ok(port);
        }
    }
    let last = highest()?;
    if !(1..=255).contains(&last) {
        return Err("父 USB Hub 未返回有效的下游端口数。".into());
    }
    let mut found = None;
    for port in 1..=last {
        if matches(port)? && found.replace(port).is_some() {
            return Err("同一父 USB Hub 上有多个端口符合目标，无法安全重新连接。".into());
        }
    }
    found.ok_or_else(|| "父 USB Hub 上没有找到与 所选声卡 的驱动键和设备描述均符合的端口。".into())
}

struct Hub(HANDLE);

impl Drop for Hub {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn win_error(label: &str, err: windows::core::Error) -> String {
    format!(
        "{label}失败（HRESULT 0x{:08X}，{}）。",
        err.code().0 as u32,
        err.message()
    )
}

impl Hub {
    fn open(path: &str, desired_access: u32) -> Result<Self> {
        unsafe {
            CreateFileW(
                &HSTRING::from(path),
                desired_access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .map(Self)
            .map_err(|e| win_error("打开 所选声卡 的父 USB Hub 接口", e))
        }
    }

    // These two METHOD_BUFFERED queries both take ConnectionIndex as the
    // first field of the full, variably sized structure. USBView uses the
    // same input/output buffer. No write operation shares this entry point.
    fn query(&self, port: u32, what: ReadQuery) -> Result<Vec<u8>> {
        let (code, capacity) = match what {
            ReadQuery::Connection => (
                IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX,
                size_of::<USB_NODE_CONNECTION_INFORMATION_EX>() + 255 * size_of::<USB_PIPE_INFO>(),
            ),
            ReadQuery::DriverKey => (IOCTL_USB_GET_NODE_CONNECTION_DRIVERKEY_NAME, 65536),
        };
        let mut data = vec![0u8; capacity];
        data[..4].copy_from_slice(&port.to_le_bytes());
        let mut returned = 0u32;
        let pointer = data.as_mut_ptr().cast();
        unsafe {
            DeviceIoControl(
                self.0,
                code,
                Some(pointer),
                capacity as u32,
                Some(pointer),
                capacity as u32,
                Some(&mut returned),
                None,
            )
            .map_err(|e| win_error("读取 所选声卡 所在 USB 端口", e))?;
        }
        if returned as usize > capacity {
            return Err("USB Hub 返回的端口信息超过接收缓冲区。".into());
        }
        data.truncate(returned as usize);
        Ok(data)
    }

    fn highest_port(&self) -> Result<u32> {
        let mut info = USB_HUB_INFORMATION_EX::default();
        let mut returned = 0;
        let buffer = (&raw mut info).cast();
        let size = size_of::<USB_HUB_INFORMATION_EX>() as u32;
        unsafe {
            DeviceIoControl(
                self.0,
                IOCTL_USB_GET_HUB_INFORMATION_EX,
                Some(buffer),
                size,
                Some(buffer),
                size,
                Some(&mut returned),
                None,
            )
            .map_err(|e| win_error("读取直接父 Hub 的端口数", e))?;
        }
        if (returned as usize)
            < offset_of!(USB_HUB_INFORMATION_EX, HighestPortNumber) + size_of::<u16>()
        {
            return Err("USB Hub 返回了不完整的端口数量信息。".into());
        }
        Ok(u32::from(info.HighestPortNumber))
    }

    fn matches(&self, port: u32, driver_key: &str, model: (u16, u16)) -> Result<bool> {
        if !is_connection(&self.query(port, ReadQuery::Connection)?, port, model)? {
            return Ok(false);
        }
        let key = self.query(port, ReadQuery::DriverKey)?;
        Ok(port_driver_key(&key, port)?.eq_ignore_ascii_case(driver_key))
    }

    fn cycle(&self, port: u32) -> Result<()> {
        let request = USB_CYCLE_PORT_PARAMS {
            ConnectionIndex: port,
            StatusReturned: 0,
        };
        let mut reply = USB_CYCLE_PORT_PARAMS::default();
        let mut returned = 0;
        let size = size_of::<USB_CYCLE_PORT_PARAMS>() as u32;
        // Some hub stacks return the USBD status in the same-sized structure.
        // The IOCTL contract also permits no output and reports status through
        // DeviceIoControl. Send exactly once for either contract.
        unsafe {
            DeviceIoControl(
                self.0,
                IOCTL_USB_HUB_CYCLE_PORT,
                Some((&raw const request).cast()),
                size,
                Some((&raw mut reply).cast()),
                size,
                Some(&mut returned),
                None,
            )
            .map_err(|e| win_error("请求重新连接 所选声卡 USB 端口", e))?;
        }
        cycle_response(port, returned, reply)
    }
}

enum ReadQuery {
    Connection,
    DriverKey,
}

fn port_driver_key(data: &[u8], port: u32) -> Result<String> {
    let start = offset_of!(USB_NODE_CONNECTION_DRIVERKEY_NAME, DriverKeyName);
    if data.len() < start || data[..4] != port.to_le_bytes() {
        return Err("USB Hub 返回了其他端口或不完整的驱动键。".into());
    }
    let actual = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    if !(start..=data.len()).contains(&actual) {
        return Err("USB Hub 返回的端口驱动键长度无效。".into());
    }
    terminated_string(&data[start..actual])
}

fn is_connection(data: &[u8], port: u32, model: (u16, u16)) -> Result<bool> {
    type Info = USB_NODE_CONNECTION_INFORMATION_EX;
    if data.len() < offset_of!(Info, PipeList) {
        return Err("USB Hub 返回了不完整的设备描述。".into());
    }
    let descriptor = offset_of!(Info, DeviceDescriptor);
    let word = |offset| u16::from_le_bytes([data[offset], data[offset + 1]]);
    let status = offset_of!(Info, ConnectionStatus);
    let state = i32::from_le_bytes(data[status..status + 4].try_into().unwrap());
    if state != DeviceConnected.0 {
        // USBHUB3 can return a zero index and no descriptor for an empty port.
        return Ok(false);
    }
    if data[..4] != port.to_le_bytes() {
        return Err("USB Hub 返回的已连接设备属于其他端口。".into());
    }
    if data[descriptor + offset_of!(USB_DEVICE_DESCRIPTOR, bLength)] as usize
        != size_of::<USB_DEVICE_DESCRIPTOR>()
        || data[descriptor + offset_of!(USB_DEVICE_DESCRIPTOR, bDescriptorType)] as u32
            != USB_DEVICE_DESCRIPTOR_TYPE
        || data[offset_of!(Info, DeviceIsHub)] != 0
        || data[descriptor + offset_of!(USB_DEVICE_DESCRIPTOR, bDeviceClass)] as u32
            == USB_DEVICE_CLASS_HUB
        || word(descriptor + offset_of!(USB_DEVICE_DESCRIPTOR, idVendor)) != model.0
        || word(descriptor + offset_of!(USB_DEVICE_DESCRIPTOR, idProduct)) != model.1
    {
        return Ok(false);
    }
    Ok(true)
}

fn cycle_response(port: u32, returned: u32, reply: USB_CYCLE_PORT_PARAMS) -> Result<()> {
    if returned == 0 {
        return Ok(());
    }
    if returned != size_of::<USB_CYCLE_PORT_PARAMS>() as u32 || reply.ConnectionIndex != port {
        return Err("USB 端口重连已请求，Hub 返回了无法核对的回执，请等待端点读回。".into());
    }
    let status = reply.StatusReturned;
    if (status as i32) < 0 {
        return Err(format!(
            "USB Hub 返回 USBD 状态 0x{status:08X}，请等待目标端点读回。"
        ));
    }
    Ok(())
}

fn require_same(expected: &PortTarget, current: &PortTarget) -> Result<()> {
    if current != expected {
        return Err("确认后 所选声卡、USB 端口或本次连接已发生变化，请重新检测。".into());
    }
    Ok(())
}

/// Produces the native confirmation evidence without requesting hub write access.
pub fn target(d: &Endpoint) -> Result<PortTarget> {
    let (target, _) = attachment(d, PortLookup::Discover, 0)?;
    Ok(target)
}

/// The caller owns the confirmation and can issue this fallback only once.
pub fn reconnect(d: &Endpoint, expected: &PortTarget) -> Result<()> {
    absent(d)?;
    let (observed, _) = attachment(d, PortLookup::Exact(expected.port), 0)?;
    require_same(expected, &observed)?;
    let (current, hub) = attachment(d, PortLookup::Exact(expected.port), GENERIC_WRITE.0)?;
    require_same(expected, &current)?;
    // Keep the same handle open and check the confirmed connection again
    // immediately before dispatch. Never search for a replacement here.
    if !hub.matches(current.port, &current.driver_key, model(&d.physical_id)?)? {
        return Err("已确认的 所选声卡 USB 端口在执行前发生变化。".into());
    }
    absent(d)?;
    hub.cycle(current.port)
}

pub(super) fn unique_target(d: &Endpoint) -> Result<PortTarget> {
    attachment(d, PortLookup::Unique, 0).map(|(target, _)| target)
}

// The write-capable handle stays open from final binding through dispatch.
// The caller rechecks the selected active endpoint and its journal here.
pub(super) fn cycle_present(
    d: &Endpoint,
    expected: &PortTarget,
    recheck: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let (now, hub) = attachment(d, PortLookup::Exact(expected.port), GENERIC_WRITE.0)?;
    require_same(expected, &now)?;
    recheck()?;
    if native::arrival(&d.function_id)? != d.arrival
        || native::arrival(&d.physical_id)? != expected.usb_arrival
        || !hub.matches(expected.port, &expected.driver_key, model(&d.physical_id)?)?
    {
        return Err("硬重启执行前，选定的 USB 连接已改变。".into());
    }
    hub.cycle(expected.port)
}

pub(super) fn returned(d: &Endpoint, previous: &PortTarget) -> Result<u64> {
    let (now, _) = attachment(d, PortLookup::Exact(previous.port), 0)?;
    let arrived = now.usb_arrival;
    if arrived == previous.usb_arrival {
        return Err("尚未读到物理 USB 重新枚举后的到达时间。".into());
    }
    let mut expected = previous.clone();
    expected.usb_arrival = arrived;
    require_same(&expected, &now)?;
    Ok(arrived)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hard_restart_requires_a_single_verified_port() {
        assert_eq!(find_port(None, || Ok(7), |p| Ok(p == 3)).unwrap(), 3);
        assert!(find_port(None, || Ok(7), |p| Ok(p == 3 || p == 7)).is_err());
        assert!(find_port(None, || Ok(7), |_| Ok(false)).is_err());
        assert!(
            find_port(
                None,
                || Ok(7),
                |p| if p == 6 {
                    Err("unreadable".into())
                } else {
                    Ok(p == 3)
                }
            )
            .is_err()
        );
    }
    #[test]
    fn port_descriptor_and_pnp_id_must_identify_the_same_device() {
        let model = model(r"USB\VID_0b05&PID_1A52\instance").unwrap();
        assert_eq!(model, (0x0b05, 0x1a52));
        let mut data = vec![0; 35];
        data[0] = 11;
        data[4..6].copy_from_slice(&[18, 1]);
        data[12..16].copy_from_slice(&[5, 11, 82, 26]);
        data[31] = 1;
        assert!(is_connection(&data, 11, model).unwrap());
        assert!(is_connection(&data, 3, model).is_err());
        assert!(!is_connection(&data, 11, (0x152a, 0x889b)).unwrap());
        data[24] = 1;
        assert!(!is_connection(&data, 11, model).unwrap());
    }
}
