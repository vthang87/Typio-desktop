//! The suggestion list: a top-most window that never takes focus from the field. Clicking a
//! row inserts it.

use std::cell::RefCell;

use typio_core::{RankedSuggestion, SuggestionKind};
use typio_platform::Rect;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateFontW, DeleteDC, DeleteObject, DrawTextW, EndPaint,
    FillRect, FrameRect, GetMonitorInfoW, GetSysColor, GetSysColorBrush, GetTextExtentPoint32W,
    InvalidateRect, MonitorFromPoint, SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, COLOR_BTNSHADOW, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT,
    COLOR_WINDOW, COLOR_WINDOWTEXT, DEFAULT_CHARSET, DRAW_TEXT_FORMAT, DT_END_ELLIPSIS,
    DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HDC, HFONT, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, LoadCursorW, RegisterClassW, SetWindowPos, ShowWindow,
    CS_DROPSHADOW, HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    WM_ERASEBKGND, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::i18n::{tr, T};

const ROW_HEIGHT: f64 = 28.0;
const PADDING: f64 = 4.0;
const GAP: f64 = 4.0;
/// Space between a row's edges and its text, on both sides.
const INSET: f64 = 10.0;
const HINT_GAP: f64 = 16.0;
const MIN_WIDTH: f64 = 240.0;
const MAX_WIDTH: f64 = 520.0;
const FONT_SIZE: f64 = 14.0;

/// What the window paints; kept apart from the controller so painting never waits for it.
struct View {
    rows: Vec<(String, String)>,
    /// A notice instead of suggestions: not clickable, nothing selected.
    message: bool,
    selected: Option<usize>,
    scale: f64,
    font: Option<(f64, HFONT)>,
}

thread_local! {
    static VIEW: RefCell<View> = const {
        RefCell::new(View {
            rows: Vec::new(),
            message: false,
            selected: None,
            scale: 1.0,
            font: None,
        })
    };
}

pub struct Popup {
    window: HWND,
    visible: bool,
    /// Screen frame in physical pixels, for telling clicks on it from clicks elsewhere.
    frame: Rect,
}

impl Popup {
    pub fn new() -> Self {
        let instance = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
        let class = WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            lpszClassName: w!("TypioPopup"),
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) };
        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("TypioPopup"),
                w!(""),
                WS_POPUP,
                0,
                0,
                10,
                10,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .unwrap_or_default();
        Self {
            window,
            visible: false,
            frame: Rect::default(),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn selected(&self) -> Option<usize> {
        if !self.visible {
            return None;
        }
        VIEW.with_borrow(|view| view.selected)
    }

    pub fn select(&mut self, index: Option<usize>) {
        VIEW.with_borrow_mut(|view| view.selected = index);
        unsafe {
            let _ = InvalidateRect(Some(self.window), None, true);
        }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        self.visible && self.frame.contains(x, y)
    }

    pub fn hide(&mut self) {
        unsafe {
            let _ = ShowWindow(self.window, SW_HIDE);
        }
        self.visible = false;
        VIEW.with_borrow_mut(|view| view.selected = None);
    }

    /// Shows `items` below `anchor` (the caret or the field), flipping above when needed.
    pub fn show(&mut self, items: &[RankedSuggestion], anchor: Rect) {
        let rows = items
            .iter()
            .map(|item| {
                let hint = match (&item.trigger, item.kind) {
                    (Some(trigger), _) => format!("{trigger} →"),
                    (None, SuggestionKind::Initials) => tr(T::InitialsHint).into(),
                    _ => String::new(),
                };
                (item.value.clone(), hint)
            })
            .collect();
        self.render(rows, false, anchor);
    }

    /// A short notice in place of the suggestions.
    pub fn show_message(&mut self, text: &str, anchor: Rect) {
        self.render(vec![(text.to_string(), String::new())], true, anchor);
    }

    fn render(&mut self, rows: Vec<(String, String)>, message: bool, anchor: Rect) {
        let point = POINT {
            x: anchor.x as i32,
            y: (anchor.y + anchor.height) as i32,
        };
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
        let (mut dpi_x, mut dpi_y) = (96, 96);
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let scale = f64::from(dpi_x) / 96.0;
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        };

        let (width, height) = VIEW.with_borrow_mut(|view| {
            let font = font_for(view, scale);
            let widest = measure(font, &rows);
            view.rows = rows;
            view.message = message;
            view.selected = None;
            view.scale = scale;
            let width = (widest + 2.0 * (INSET + PADDING) * scale)
                .clamp(MIN_WIDTH * scale, MAX_WIDTH * scale);
            let height = view.rows.len() as f64 * ROW_HEIGHT * scale + 2.0 * PADDING * scale;
            (width.round(), height.round())
        });

        let gap = GAP * scale;
        let below = anchor.y + anchor.height + gap;
        let y = if below + height > f64::from(work.bottom) {
            (anchor.y - gap - height).max(f64::from(work.top))
        } else {
            below
        };
        let x = anchor
            .x
            .min(f64::from(work.right) - width)
            .max(f64::from(work.left));
        self.frame = Rect {
            x,
            y,
            width,
            height,
        };
        unsafe {
            let _ = SetWindowPos(
                self.window,
                Some(HWND_TOPMOST),
                x as i32,
                y as i32,
                width as i32,
                height as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(self.window), None, true);
        }
        self.visible = true;
    }
}

/// Segoe UI at `scale`, made once per scale.
fn font_for(view: &mut View, scale: f64) -> HFONT {
    if let Some((made_for, font)) = view.font {
        if made_for == scale {
            return font;
        }
        unsafe {
            let _ = DeleteObject(font.into());
        }
    }
    let font = unsafe {
        CreateFontW(
            -((FONT_SIZE * scale).round() as i32),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        )
    };
    view.font = Some((scale, font));
    font
}

/// The widest row: its value, and its hint after a gap.
fn measure(font: HFONT, rows: &[(String, String)]) -> f64 {
    let dc = unsafe { CreateCompatibleDC(None) };
    let previous = unsafe { SelectObject(dc, font.into()) };
    let width_of = |text: &str| -> f64 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        let _ = unsafe { GetTextExtentPoint32W(dc, &wide, &mut size) };
        f64::from(size.cx)
    };
    let widest = rows
        .iter()
        .map(|(value, hint)| {
            let hint_width = if hint.is_empty() {
                0.0
            } else {
                HINT_GAP + width_of(hint)
            };
            width_of(value) + hint_width
        })
        .fold(0.0, f64::max);
    unsafe {
        SelectObject(dc, previous);
        let _ = DeleteDC(dc);
    }
    widest
}

fn paint(window: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let dc = unsafe { BeginPaint(window, &mut paint) };
    VIEW.with(|cell| {
        if let Ok(view) = cell.try_borrow() {
            draw(dc, window, &view);
        }
    });
    unsafe {
        let _ = EndPaint(window, &paint);
    }
}

fn draw(dc: HDC, window: HWND, view: &View) {
    let mut client = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(window, &mut client);
        FillRect(dc, &client, GetSysColorBrush(COLOR_WINDOW));
        FrameRect(dc, &client, GetSysColorBrush(COLOR_BTNSHADOW));
        SetBkMode(dc, TRANSPARENT);
    }
    let Some((_, font)) = view.font else {
        return;
    };
    let previous = unsafe { SelectObject(dc, font.into()) };
    let scale = view.scale;
    let padding = (PADDING * scale).round() as i32;
    let inset = (INSET * scale).round() as i32;
    let row_height = ROW_HEIGHT * scale;
    for (index, (value, hint)) in view.rows.iter().enumerate() {
        let top = padding + (index as f64 * row_height).round() as i32;
        let row = RECT {
            left: padding,
            top,
            right: client.right - padding,
            bottom: top + row_height.round() as i32,
        };
        let selected = !view.message && view.selected == Some(index);
        let (text_color, hint_color) = if selected {
            unsafe { FillRect(dc, &row, GetSysColorBrush(COLOR_HIGHLIGHT)) };
            let color = unsafe { GetSysColor(COLOR_HIGHLIGHTTEXT) };
            (color, color)
        } else {
            unsafe { (GetSysColor(COLOR_WINDOWTEXT), GetSysColor(COLOR_GRAYTEXT)) }
        };
        let mut text = RECT {
            left: row.left + inset,
            right: row.right - inset,
            ..row
        };
        if !hint.is_empty() {
            draw_text(dc, hint, &mut text.clone(), hint_color, DT_RIGHT);
            let hint_width = measure(font, &[(hint.clone(), String::new())]);
            text.right -= (hint_width + HINT_GAP * scale).round() as i32;
        }
        draw_text(dc, value, &mut text, text_color, DT_END_ELLIPSIS);
    }
    unsafe { SelectObject(dc, previous) };
}

fn draw_text(dc: HDC, text: &str, rect: &mut RECT, color: u32, extra: DRAW_TEXT_FORMAT) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        SetTextColor(dc, COLORREF(color));
        DrawTextW(
            dc,
            &mut wide,
            rect,
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | extra,
        );
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(window);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let y = f64::from((lparam.0 >> 16) as i16);
            let row = VIEW.with(|cell| {
                let view = cell.try_borrow().ok()?;
                if view.message {
                    return None;
                }
                let row = ((y - PADDING * view.scale) / (ROW_HEIGHT * view.scale)).floor();
                (row >= 0.0 && (row as usize) < view.rows.len()).then_some(row as usize)
            });
            if let Some(row) = row {
                super::controller::post(super::controller::Command::Insert(row));
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
