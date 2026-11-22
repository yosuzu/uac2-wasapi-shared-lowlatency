//! Standard HWND controls expose the recovery workflow to keyboards and UI Automation.
//! The parent paints the desktop theme, including its own caption and action buttons.
use std::{
    cell::Cell,
    ffi::c_void,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::*,
            HiDpi::{GetDpiForSystem, GetDpiForWindow},
            Input::KeyboardAndMouse::{
                EnableWindow, GetDoubleClickTime, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN,
            },
            WindowsAndMessaging::*,
        },
    },
    core::{Error, HSTRING, PCWSTR, Result, w},
};

const MIN: u32 = 101;
const MAX: u32 = 102;
const CLOSE: u32 = 103;
const SAFE: u32 = 110;
const NEXT: u32 = 111;
const CONFIRM: u32 = 112;
const LIST: u32 = 120;
const HEAD: u32 = 121;
const BODY: u32 = 122;
const ACTION: u32 = WM_APP + 72;
const CLASS: PCWSTR = w!("UAC2.Control.Recovery");

pub(super) enum Action {
    Next,
    Confirm,
    Back,
    Close,
    Select(Option<usize>),
    Tick,
}

fn color(hex: u32) -> COLORREF {
    COLORREF(((hex & 255) << 16) | (hex & 0xff00) | (hex >> 16))
}
struct Theme {
    bg: COLORREF,
    card: COLORREF,
    edge: COLORREF,
    text: COLORREF,
    muted: COLORREF,
    soft: COLORREF,
    hover: COLORREF,
    accent: COLORREF,
    pink: COLORREF,
    danger: COLORREF,
}
impl Theme {
    fn new(light: bool) -> Self {
        let pick = |d, l| color(if light { l } else { d });
        Self {
            bg: pick(0x14171d, 0xeaf0f5),
            card: pick(0x20242c, 0xf7fafd),
            edge: pick(0x3d4551, 0xcad5e0),
            text: pick(0xeef3f7, 0x193047),
            muted: pick(0xbbc5ce, 0x475569),
            soft: pick(0x2e343e, 0xedf2f7),
            hover: pick(0x373e49, 0xe2ebf3),
            accent: pick(0x83f4d7, 0x00685f),
            pink: color(0xc41d64),
            danger: pick(0xb33443, 0xb42f41),
        }
    }
}

// Only the presentation attributes are shared with the re-entrant window procedure.
// Device choices and background results stay in the outer message loop.
struct Chrome {
    theme: Theme,
    en: bool,
    icon: HICON,
    dpi: Cell<i32>,
    fonts: [Cell<HFONT>; 3],
    card_brush: HBRUSH,
    list_visible: Cell<bool>,
    danger: Cell<bool>,
}
impl Chrome {
    fn px(&self, n: i32) -> i32 {
        (n * self.dpi.get() + 48) / 96
    }
    fn refresh_fonts(&self, hwnd: HWND) {
        unsafe {
            self.dpi.set(GetDpiForWindow(hwnd).max(96) as i32);
            for (i, (size, weight)) in [(15, 400), (24, 700), (28, 700)].into_iter().enumerate() {
                let font = CreateFontW(
                    -self.px(size),
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    0,
                    w!("Segoe UI"),
                );
                let old = self.fonts[i].replace(font);
                if i < 2 {
                    for id in [HEAD, BODY, LIST, SAFE, NEXT, CONFIRM] {
                        if (id == HEAD) == (i == 1)
                            && let Ok(child) = GetDlgItem(Some(hwnd), id as i32)
                        {
                            SendMessageW(
                                child,
                                WM_SETFONT,
                                Some(WPARAM(font.0 as usize)),
                                Some(LPARAM(1)),
                            );
                        }
                    }
                }
                if !old.0.is_null() {
                    let _ = DeleteObject(old.into());
                }
            }
        }
    }
    fn card(&self, hwnd: HWND) -> RECT {
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut r);
        }
        let gutter = self.px(18);
        RECT {
            left: gutter,
            top: self.px(66),
            right: r.right - gutter,
            bottom: r.bottom - gutter,
        }
    }
    fn layout(&self, hwnd: HWND) {
        let r = self.card(hwnd);
        let p = |v| self.px(v);
        let left = r.left + p(23);
        let right = r.right - p(23);
        let bottom = r.bottom - p(18);
        let move_to = |id: u32, x, y, w: i32, h: i32| unsafe {
            if let Ok(c) = GetDlgItem(Some(hwnd), id as i32) {
                let _ = MoveWindow(c, x, y, w.max(1), h.max(1), true);
            }
        };
        let width = r.right + p(18);
        for (i, id) in [MIN, MAX, CLOSE].into_iter().enumerate() {
            move_to(id, width - (3 - i as i32) * p(44), 0, p(44), p(54));
        }
        move_to(HEAD, left, r.top + p(20), right - left, p(35));
        let y = r.top + p(66);
        let body_bottom = bottom - p(57);
        move_to(
            BODY,
            left,
            y,
            right - left,
            if self.list_visible.get() {
                p(39)
            } else {
                body_bottom - y
            },
        );
        unsafe {
            if let Ok(body) = GetDlgItem(Some(hwnd), BODY as i32) {
                let _ = ShowScrollBar(body, SB_VERT, false);
                let mut format = RECT::default();
                SendMessageW(
                    body,
                    EM_GETRECT,
                    None,
                    Some(LPARAM((&raw mut format) as isize)),
                );
                let lines = SendMessageW(body, EM_GETLINECOUNT, None, None).0 as i32;
                let dc = GetDC(Some(body));
                let old = SelectObject(dc, self.fonts[0].get().into());
                let mut metrics = TEXTMETRICW::default();
                let measured = GetTextMetricsW(dc, &mut metrics).as_bool();
                SelectObject(dc, old);
                ReleaseDC(Some(body), dc);
                if measured && lines.saturating_mul(metrics.tmHeight) > format.bottom - format.top {
                    let _ = ShowScrollBar(body, SB_VERT, true);
                }
            }
        }
        if self.list_visible.get() {
            move_to(LIST, left, y + p(46), right - left, body_bottom - y - p(46));
        }
        let active = if self.danger.get() { CONFIRM } else { NEXT };
        let next_visible = unsafe {
            GetDlgItem(Some(hwnd), active as i32).is_ok_and(|w| IsWindowVisible(w).as_bool())
        };
        let next_width = p(if self.en { 168 } else { 142 });
        move_to(
            active,
            right - next_width,
            bottom - p(38),
            next_width,
            p(38),
        );
        let safe_width = p(if self.en { 90 } else { 82 });
        move_to(
            SAFE,
            right - safe_width - if next_visible { next_width + p(9) } else { 0 },
            bottom - p(38),
            safe_width,
            p(38),
        );
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
    fn background(&self, hwnd: HWND, dc: HDC) {
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut r);
            fill(dc, &r, self.theme.bg);
            let card = self.card(hwnd);
            rounded(dc, &card, self.theme.card, self.theme.edge, self.px(38));
            let icon = self.px(40);
            if !self.icon.0.is_null() {
                let _ = DrawIconEx(
                    dc,
                    self.px(20),
                    self.px(9),
                    self.icon,
                    icon,
                    icon,
                    0,
                    None,
                    DI_NORMAL,
                );
            }
            let mut title = RECT {
                left: self.px(70),
                top: 0,
                right: r.right - self.px(139),
                bottom: self.px(58),
            };
            label(
                dc,
                &mut title,
                if self.en {
                    "UAC2 Shared Period"
                } else {
                    "UAC2 共享周期"
                },
                self.fonts[2].get(),
                self.theme.text,
                DT_LEFT,
            );
        }
    }
    fn draw_item(&self, hwnd: HWND, item: &DRAWITEMSTRUCT) {
        let has = |flag: ODS_FLAGS| item.itemState.0 & flag.0 != 0;
        let id = item.CtlID;
        let dc = item.hDC;
        let mut r = item.rcItem;
        let p = &self.theme;
        if id == LIST {
            let selected = has(ODS_SELECTED);
            fill(dc, &r, if selected { p.soft } else { p.card });
            if item.itemID != u32::MAX {
                let idx = WPARAM(item.itemID as usize);
                unsafe {
                    let n = SendMessageW(item.hwndItem, LB_GETTEXTLEN, Some(idx), None).0;
                    if (0..65536).contains(&n) {
                        let mut text = vec![0u16; n as usize + 1];
                        SendMessageW(
                            item.hwndItem,
                            LB_GETTEXT,
                            Some(idx),
                            Some(LPARAM(text.as_mut_ptr() as isize)),
                        );
                        r.left += self.px(10);
                        label(
                            dc,
                            &mut r,
                            &String::from_utf16_lossy(&text[..n as usize]),
                            self.fonts[0].get(),
                            if selected { p.accent } else { p.text },
                            DT_LEFT,
                        );
                    }
                }
            }
            if has(ODS_FOCUS) {
                unsafe {
                    let _ = DrawFocusRect(dc, &item.rcItem);
                }
            }
            return;
        }
        let caption = matches!(id, MIN | MAX | CLOSE);
        let disabled = has(ODS_DISABLED);
        let hot = has(ODS_SELECTED) || has(ODS_HOTLIGHT);
        let focused = has(ODS_FOCUS);
        fill(dc, &r, if caption { p.bg } else { p.card });
        if caption {
            if hot {
                fill(dc, &r, if id == CLOSE { p.danger } else { p.hover });
            }
            let symbol = match id {
                MIN => "\u{e921}",
                MAX if unsafe { IsZoomed(hwnd).as_bool() } => "\u{e923}",
                MAX => "\u{e922}",
                _ => "\u{e8bb}",
            };
            unsafe {
                let font = CreateFontW(
                    -self.px(13),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    0,
                    w!("Segoe MDL2 Assets"),
                );
                label(
                    dc,
                    &mut r,
                    symbol,
                    font,
                    if disabled { p.muted } else { p.text },
                    DT_CENTER,
                );
                let _ = DeleteObject(font.into());
            }
        } else {
            let strong = matches!(id, NEXT | CONFIRM) && !disabled;
            let bg = if strong {
                if self.danger.get() { p.danger } else { p.pink }
            } else if hot {
                p.hover
            } else {
                p.soft
            };
            r.left += 2;
            r.right -= 2;
            r.top += 2;
            r.bottom -= 2;
            rounded(
                dc,
                &r,
                bg,
                if focused { p.accent } else { p.edge },
                self.px(36),
            );
            let mut buf = [0u16; 160];
            unsafe {
                let n = GetWindowTextW(item.hwndItem, &mut buf) as usize;
                label(
                    dc,
                    &mut r,
                    &String::from_utf16_lossy(&buf[..n]),
                    self.fonts[0].get(),
                    if strong {
                        color(0xffffff)
                    } else if disabled {
                        p.muted
                    } else {
                        p.text
                    },
                    DT_CENTER,
                );
            }
        }
        if focused && caption {
            unsafe {
                let _ = DrawFocusRect(dc, &r);
            }
        }
    }
}
impl Drop for Chrome {
    fn drop(&mut self) {
        unsafe {
            for f in &self.fonts {
                let _ = DeleteObject(f.get().into());
            }
            let _ = DeleteObject(self.card_brush.into());
        }
    }
}

fn fill(dc: HDC, r: &RECT, color: COLORREF) {
    unsafe {
        SetDCBrushColor(dc, color);
        FillRect(dc, r, HBRUSH(GetStockObject(DC_BRUSH).0));
    }
}
fn rounded(dc: HDC, r: &RECT, bg: COLORREF, edge: COLORREF, diameter: i32) {
    unsafe {
        let old_brush = SelectObject(dc, GetStockObject(DC_BRUSH));
        let old_pen = SelectObject(dc, GetStockObject(DC_PEN));
        SetDCBrushColor(dc, bg);
        SetDCPenColor(dc, edge);
        let _ = RoundRect(dc, r.left, r.top, r.right, r.bottom, diameter, diameter);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
    }
}
fn label(dc: HDC, r: &mut RECT, text: &str, font: HFONT, fg: COLORREF, align: DRAW_TEXT_FORMAT) {
    // Changing pages clears a button caption before hiding it. USER can synchronously
    // request a paint for that empty caption, which has no drawable UTF-16 buffer.
    if text.is_empty() {
        return;
    }
    unsafe {
        let old = SelectObject(dc, font.into());
        SetTextColor(dc, fg);
        SetBkMode(dc, TRANSPARENT);
        DrawTextW(
            dc,
            &mut text.encode_utf16().collect::<Vec<_>>(),
            r,
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS | align,
        );
        SelectObject(dc, old);
    }
}

unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let info = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, info.lpCreateParams as isize);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Chrome;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let chrome = &*ptr;
        match msg {
            WM_NCCALCSIZE if wp.0 != 0 => return LRESULT(0),
            WM_GETMINMAXINFO => {
                let info = &mut *(lp.0 as *mut MINMAXINFO);
                info.ptMinTrackSize = POINT {
                    x: chrome.px(520),
                    y: chrome.px(385),
                };
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut m = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(monitor, &mut m).as_bool() {
                    info.ptMaxPosition = POINT {
                        x: m.rcWork.left - m.rcMonitor.left,
                        y: m.rcWork.top - m.rcMonitor.top,
                    };
                    info.ptMaxSize = POINT {
                        x: m.rcWork.right - m.rcWork.left,
                        y: m.rcWork.bottom - m.rcWork.top,
                    };
                }
                return LRESULT(0);
            }
            WM_NCHITTEST => {
                let mut xy = POINT {
                    x: lp.0 as i16 as i32,
                    y: (lp.0 >> 16) as i16 as i32,
                };
                let _ = ScreenToClient(hwnd, &mut xy);
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let b = chrome.px(6);
                let edge = if IsZoomed(hwnd).as_bool() {
                    0
                } else {
                    match (
                        xy.x < b,
                        xy.x >= rect.right - b,
                        xy.y < b,
                        xy.y >= rect.bottom - b,
                    ) {
                        (true, _, true, _) => HTTOPLEFT,
                        (_, true, true, _) => HTTOPRIGHT,
                        (true, _, _, true) => HTBOTTOMLEFT,
                        (_, true, _, true) => HTBOTTOMRIGHT,
                        (true, _, _, _) => HTLEFT,
                        (_, true, _, _) => HTRIGHT,
                        (_, _, true, _) => HTTOP,
                        (_, _, _, true) => HTBOTTOM,
                        _ => 0,
                    }
                };
                return LRESULT(if edge != 0 {
                    edge
                } else if xy.y < chrome.px(56) {
                    HTCAPTION
                } else {
                    HTCLIENT
                } as isize);
            }
            WM_DPICHANGED => {
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                chrome.refresh_fonts(hwnd);
                chrome.layout(hwnd);
                return LRESULT(0);
            }
            WM_SIZE => {
                chrome.layout(hwnd);
                return LRESULT(0);
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                chrome.background(hwnd, dc);
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }
            WM_PRINTCLIENT => {
                chrome.background(hwnd, HDC(wp.0 as _));
                return LRESULT(0);
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_DRAWITEM => {
                chrome.draw_item(hwnd, &*(lp.0 as *const DRAWITEMSTRUCT));
                return LRESULT(1);
            }
            WM_MEASUREITEM => {
                (*(lp.0 as *mut MEASUREITEMSTRUCT)).itemHeight = chrome.px(36) as u32;
                return LRESULT(1);
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                let dc = HDC(wp.0 as _);
                SetBkColor(dc, chrome.theme.card);
                SetTextColor(
                    dc,
                    if GetDlgCtrlID(HWND(lp.0 as _)) == BODY as i32 {
                        chrome.theme.muted
                    } else {
                        chrome.theme.text
                    },
                );
                return LRESULT(chrome.card_brush.0 as isize);
            }
            WM_CLOSE => {
                let _ = PostMessageW(Some(hwnd), ACTION, WPARAM(CLOSE as usize), LPARAM(0));
                return LRESULT(0);
            }
            WM_COMMAND => {
                let id = wp.0 as u16 as u32;
                let event = (wp.0 >> 16) as u32;
                if (id == LIST && event == LBN_SELCHANGE)
                    || (matches!(id, MIN | MAX | CLOSE | SAFE | NEXT | CONFIRM)
                        && event == BN_CLICKED)
                    || id == IDCANCEL.0 as u32
                {
                    let _ = PostMessageW(Some(hwnd), ACTION, WPARAM(id as usize), LPARAM(0));
                }
                return LRESULT(0);
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

pub(super) struct Window {
    hwnd: HWND,
    chrome: Box<Chrome>,
    arm_confirm: Cell<Option<Instant>>,
}
impl Window {
    pub(super) fn open(en: bool, light: bool) -> Result<Self> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)?.into();
            let icon = LoadIconW(Some(instance), PCWSTR(101 as _))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))?;
            let theme = Theme::new(light);
            let chrome = Box::new(Chrome {
                card_brush: CreateSolidBrush(theme.card),
                theme,
                en,
                icon,
                dpi: Cell::new(GetDpiForSystem().max(96) as i32),
                fonts: Default::default(),
                list_visible: Cell::new(false),
                danger: Cell::new(false),
            });
            let wc = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: CLASS,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                hIcon: icon,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err(Error::from_thread());
            }
            let hwnd = CreateWindowExW(
                WS_EX_APPWINDOW | WS_EX_CONTROLPARENT,
                CLASS,
                w!("UAC2 Control"),
                WS_POPUP
                    | WS_CLIPCHILDREN
                    | WS_THICKFRAME
                    | WS_SYSMENU
                    | WS_MINIMIZEBOX
                    | WS_MAXIMIZEBOX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                chrome.px(690),
                chrome.px(435),
                None,
                None,
                Some(instance),
                Some((&*chrome as *const Chrome).cast::<c_void>()),
            )?;
            let win = Self {
                hwnd,
                chrome,
                arm_confirm: Cell::new(None),
            };
            let child = |id: u32, class: PCWSTR, name: &str, style: WINDOW_STYLE| {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    class,
                    &HSTRING::from(name),
                    WS_CHILD | style,
                    0,
                    0,
                    1,
                    1,
                    Some(hwnd),
                    Some(HMENU(id as usize as _)),
                    Some(instance),
                    None,
                )
            };
            let btn = WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP;
            for (id, name) in [
                (MIN, if en { "Minimize" } else { "最小化" }),
                (
                    MAX,
                    if en {
                        "Maximize or restore"
                    } else {
                        "最大化或还原"
                    },
                ),
                (CLOSE, if en { "Close window" } else { "关闭窗口" }),
            ] {
                child(id, w!("BUTTON"), name, btn | WS_VISIBLE)?;
            }
            child(HEAD, w!("STATIC"), "", WS_VISIBLE | WINDOW_STYLE(0x80))?; // SS_NOPREFIX
            child(
                BODY,
                w!("EDIT"),
                "",
                WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
            )?;
            child(
                LIST,
                w!("LISTBOX"),
                "",
                WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE(
                        (LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT)
                            as u32,
                    ),
            )?;
            child(SAFE, w!("BUTTON"), "", btn)?;
            child(NEXT, w!("BUTTON"), "", btn)?;
            child(CONFIRM, w!("BUTTON"), "", btn)?;
            if !light {
                for id in [BODY, LIST] {
                    let _ = SetWindowTheme(win.child(id), w!("DarkMode_Explorer"), PCWSTR::null());
                }
            }
            win.chrome.refresh_fonts(hwnd);
            if SetTimer(Some(hwnd), 1, 120, None) == 0 {
                return Err(Error::from_thread());
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            Ok(win)
        }
    }
    fn child(&self, id: u32) -> HWND {
        unsafe { GetDlgItem(Some(self.hwnd), id as i32).unwrap_or_default() }
    }
    pub(super) fn enable_next(&self, value: bool) {
        unsafe {
            let _ = EnableWindow(self.child(NEXT), value);
        }
    }
    pub(super) fn show_page(
        &self,
        head: &str,
        body: &str,
        safe: &str,
        next: &str,
        list: bool,
        danger: bool,
        working: bool,
        enable_next: bool,
    ) {
        self.chrome.list_visible.set(list);
        self.chrome.danger.set(danger);
        unsafe {
            for (id, text) in [
                (HEAD, head),
                (BODY, body),
                (SAFE, safe),
                (NEXT, if danger { "" } else { next }),
                (CONFIRM, if danger { next } else { "" }),
            ] {
                let child = self.child(id);
                let _ = SetWindowTextW(
                    child,
                    &HSTRING::from(text.replace('\n', "\r\n").replace("\r\r\n", "\r\n")),
                );
                let _ = ShowWindow(child, if text.is_empty() { SW_HIDE } else { SW_SHOW });
            }
            let _ = ShowWindow(self.child(LIST), if list { SW_SHOW } else { SW_HIDE });
            self.arm_confirm.set(
                danger.then(|| {
                    Instant::now() + Duration::from_millis(u64::from(GetDoubleClickTime()))
                }),
            );
            let _ = EnableWindow(self.child(CONFIRM), false);
            self.enable_next(enable_next);
            let _ = EnableWindow(self.child(CLOSE), !working);
            self.chrome.layout(self.hwnd);
            let _ = SetFocus(Some(self.child(if working { BODY } else { SAFE })));
            let _ = UpdateWindow(self.hwnd);
        }
    }
    pub(super) fn set_choices(&self, items: impl Iterator<Item = String>) {
        unsafe {
            let list = self.child(LIST);
            SendMessageW(list, LB_RESETCONTENT, None, None);
            for item in items {
                let text = HSTRING::from(item);
                SendMessageW(
                    list,
                    LB_ADDSTRING,
                    None,
                    Some(LPARAM(text.as_ptr() as isize)),
                );
            }
            SendMessageW(list, LB_SETCURSEL, Some(WPARAM(usize::MAX)), None);
        }
    }
    pub(super) fn next_action(&self) -> Result<Option<Action>> {
        unsafe {
            loop {
                let mut msg = MSG::default();
                let read = GetMessageW(&mut msg, None, 0, 0).0;
                if read == -1 {
                    return Err(Error::from_thread());
                }
                if read == 0 {
                    return Ok(None);
                }
                if msg.hwnd == self.hwnd && msg.message == WM_TIMER {
                    if self
                        .arm_confirm
                        .get()
                        .is_some_and(|time| Instant::now() >= time)
                    {
                        self.arm_confirm.set(None);
                        let _ = EnableWindow(self.child(CONFIRM), true);
                    }
                    return Ok(Some(Action::Tick));
                }
                if msg.hwnd == self.hwnd && msg.message == ACTION {
                    match msg.wParam.0 as u32 {
                        MIN => {
                            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
                        }
                        MAX => {
                            let cmd = if IsZoomed(self.hwnd).as_bool() {
                                SW_RESTORE
                            } else {
                                SW_MAXIMIZE
                            };
                            let _ = ShowWindow(self.hwnd, cmd);
                        }
                        CLOSE => return Ok(Some(Action::Close)),
                        SAFE | 2 => return Ok(Some(Action::Back)),
                        NEXT => return Ok(Some(Action::Next)),
                        CONFIRM if self.chrome.danger.get() && self.arm_confirm.get().is_none() => {
                            return Ok(Some(Action::Confirm));
                        }
                        LIST => {
                            let i = SendMessageW(self.child(LIST), LB_GETCURSEL, None, None).0;
                            return Ok(Some(Action::Select(usize::try_from(i).ok())));
                        }
                        _ => {}
                    }
                    continue;
                }
                if msg.message == WM_KEYDOWN {
                    if msg.wParam.0 == VK_ESCAPE.0 as usize {
                        return Ok(Some(Action::Back));
                    }
                    if msg.wParam.0 == VK_RETURN.0 as usize {
                        if msg.lParam.0 & (1 << 30) != 0 {
                            continue;
                        }
                        let id = GetDlgCtrlID(GetFocus());
                        let target =
                            if matches!(id as u32, MIN | MAX | CLOSE | NEXT | CONFIRM | SAFE) {
                                id as u32
                            } else {
                                SAFE
                            };
                        let _ = PostMessageW(
                            Some(self.hwnd),
                            ACTION,
                            WPARAM(target as usize),
                            LPARAM(0),
                        );
                        continue;
                    }
                }
                if !IsDialogMessageW(self.hwnd, &msg).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), 1);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
