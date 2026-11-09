//! The Settings window: General, Datasets, Apps and Extension tabs, rebuilt from the app
//! state whenever it changes.

use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetSysColor, GetSysColorBrush, SetBkColor, SetTextColor, COLOR_GRAYTEXT,
    COLOR_WINDOW, COLOR_WINDOWTEXT, HDC, HFONT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    BST_CHECKED, LVCF_WIDTH, LVCOLUMNW, LVIF_STATE, LVIF_TEXT, LVIS_STATEIMAGEMASK, LVITEMW,
    LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVM_SETITEMSTATE, LVM_SETITEMTEXTW, LVNI_SELECTED, LVN_ITEMCHANGED, LVS_EX_CHECKBOXES,
    LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_NOCOLUMNHEADER, LVS_REPORT, LVS_SHOWSELALWAYS,
    LVS_SINGLESEL, NMHDR, NMLISTVIEW, NM_DBLCLK, TCIF_TEXT, TCITEMW, TCM_ADJUSTRECT, TCM_GETCURSEL,
    TCM_INSERTITEMW, TCM_SETCURSEL, TCN_SELCHANGE, WC_LISTVIEWW, WC_TABCONTROLW,
};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics, IsWindow,
    LoadCursorW, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow,
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CBN_SELCHANGE, CBS_DROPDOWNLIST,
    CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CS_HREDRAW, CS_VREDRAW, HICON, HWND_TOP, IDC_ARROW,
    LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, LB_ADDSTRING, LB_GETCURSEL, SM_CXSCREEN, SM_CYSCREEN,
    SWP_NOZORDER, SW_RESTORE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_NOTIFY, WNDCLASSW, WS_BORDER,
    WS_CAPTION, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT,
    WS_MINIMIZEBOX, WS_SYSMENU, WS_TABSTOP, WS_VSCROLL,
};

use super::controller::{with_app, UiState};
use super::{editor, ui};
use crate::i18n::{tr, trf, Language, T};
use crate::store::{AppMode, TRIGGERS};

/// `SS_LEFT`: left-aligned static text that wraps.
const STATIC_LEFT: WINDOW_STYLE = WINDOW_STYLE(0);
const WIDTH: f64 = 600.0;
const HEIGHT: f64 = 500.0;
const MARGIN: f64 = 12.0;
pub const EXTENSION_TAB: usize = 3;

const STORE_URL: &str =
    "https://chromewebstore.google.com/detail/typio/ebndieeiallcbgjbhhkbgnegmfapagjf";
pub const WEBSITE_URL: &str = "https://typio.thangdv.com/";

// Control ids.
const ID_ENABLE: usize = 100;
const ID_TRIGGER: usize = 101;
const ID_LANGUAGE: usize = 102;
const ID_DATASETS: usize = 110;
const ID_NEW: usize = 111;
const ID_EDIT: usize = 112;
const ID_DELETE: usize = 113;
const ID_IMPORT: usize = 114;
const ID_EXPORT: usize = 115;
const ID_DATA_FOLDER: usize = 116;
const ID_MODE: usize = 120;
const ID_APPS: usize = 121;
const ID_ADD_APP: usize = 122;
const ID_REMOVE_APP: usize = 123;
const ID_STORE: usize = 130;
const ID_WEBSITE: usize = 131;

const CBN_SELCHANGE_CODE: u32 = CBN_SELCHANGE;

struct Settings {
    window: HWND,
    tabs: HWND,
    /// Holds the current tab's controls; replaced when the tab or the state changes.
    page: HWND,
    tab: usize,
    scale: f64,
    font: HFONT,
    /// Controls drawn in grey.
    secondary: Vec<HWND>,
    /// The state the page shows, so an unchanged state does not rebuild it.
    shown: Option<UiState>,
    /// True while the dataset list is filled, so its check boxes do not count as clicks.
    filling: bool,
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

fn is_open() -> bool {
    SETTINGS.with_borrow(|slot| {
        slot.as_ref()
            .is_some_and(|settings| unsafe { IsWindow(Some(settings.window)) }.as_bool())
    })
}

fn settings_window() -> Option<HWND> {
    SETTINGS.with_borrow(|slot| slot.as_ref().map(|settings| settings.window))
}

/// Whether `window` is the Settings window, for keyboard navigation in the message loop.
pub fn window() -> Option<HWND> {
    settings_window()
}

/// Shows the Settings window, on `tab` when given.
pub fn show(tab: Option<usize>) {
    if !is_open() {
        create();
    }
    let Some(window) = settings_window() else {
        return;
    };
    if let Some(tab) = tab {
        select_tab(tab);
    }
    unsafe {
        let _ = ShowWindow(window, SW_RESTORE);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
}

/// Brings the window up to date after the state changed.
pub fn update(state: &UiState) {
    let changed = SETTINGS.with_borrow(|slot| {
        slot.as_ref()
            .is_some_and(|settings| settings.shown.as_ref() != Some(state))
    });
    if changed {
        rebuild_page(state);
    }
}

fn create() {
    let instance = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    let icon: HICON = ui::app_icon();
    let background = unsafe { GetSysColorBrush(COLOR_WINDOW) };
    for (class, procedure) in [
        (w!("TypioSettings"), window_proc as WindowProc),
        (w!("TypioSettingsPage"), page_proc as WindowProc),
    ] {
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            hIcon: icon,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            hbrBackground: background,
            lpszClassName: class,
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) };
    }
    let title = ui::wide(tr(T::SettingsTitle));
    let style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
    let Ok(window) = (unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("TypioSettings"),
            PCWSTR(title.as_ptr()),
            style,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }) else {
        return;
    };
    let scale = f64::from(unsafe { GetDpiForWindow(window) }.max(96)) / 96.0;
    place(window, scale, None);
    let font = ui::ui_font(scale);
    let tabs = ui::control(
        window,
        WC_TABCONTROLW,
        "",
        WS_CLIPSIBLINGS | WS_TABSTOP,
        WINDOW_EX_STYLE(0),
        0,
        (MARGIN, MARGIN, WIDTH - 2.0 * MARGIN, HEIGHT - 2.0 * MARGIN),
        scale,
        font,
    );
    for (index, title) in [T::TabGeneral, T::TabDatasets, T::TabApps, T::TabExtension]
        .into_iter()
        .enumerate()
    {
        let mut text = ui::wide(tr(title));
        let item = TCITEMW {
            mask: TCIF_TEXT,
            pszText: windows::core::PWSTR(text.as_mut_ptr()),
            ..Default::default()
        };
        unsafe {
            SendMessageW(
                tabs,
                TCM_INSERTITEMW,
                Some(WPARAM(index)),
                Some(LPARAM(&item as *const _ as isize)),
            );
        }
    }
    SETTINGS.with_borrow_mut(|slot| {
        *slot = Some(Settings {
            window,
            tabs,
            page: HWND::default(),
            tab: 0,
            scale,
            font,
            secondary: Vec::new(),
            shown: None,
            filling: false,
        });
    });
    if let Some(state) = with_app(|app| app.ui_state()) {
        rebuild_page(&state);
    }
}

type WindowProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// Sizes the window for `scale`, centred on the screen or in `suggested`.
fn place(window: HWND, scale: f64, suggested: Option<RECT>) {
    let mut frame = RECT {
        left: 0,
        top: 0,
        right: (WIDTH * scale).round() as i32,
        bottom: (HEIGHT * scale).round() as i32,
    };
    let style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
    unsafe {
        let _ = AdjustWindowRectExForDpi(
            &mut frame,
            style,
            false,
            WS_EX_CONTROLPARENT,
            (scale * 96.0) as u32,
        );
    }
    let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
    let (x, y) = match suggested {
        Some(rect) => (rect.left, rect.top),
        None => unsafe {
            (
                (GetSystemMetrics(SM_CXSCREEN) - width) / 2,
                (GetSystemMetrics(SM_CYSCREEN) - height) / 2,
            )
        },
    };
    unsafe {
        let _ = SetWindowPos(window, Some(HWND_TOP), x, y, width, height, SWP_NOZORDER);
    }
}

fn select_tab(tab: usize) {
    let tabs = SETTINGS.with_borrow_mut(|slot| {
        let settings = slot.as_mut()?;
        settings.tab = tab;
        settings.shown = None;
        Some(settings.tabs)
    });
    if let Some(tabs) = tabs {
        unsafe {
            SendMessageW(tabs, TCM_SETCURSEL, Some(WPARAM(tab)), None);
        }
    }
    if let Some(state) = with_app(|app| app.ui_state()) {
        rebuild_page(&state);
    }
}

/// Replaces the page with the current tab's controls for `state`.
fn rebuild_page(state: &UiState) {
    let Some((window, tabs, old_page, tab, scale, font)) = SETTINGS.with_borrow(|slot| {
        let settings = slot.as_ref()?;
        Some((
            settings.window,
            settings.tabs,
            settings.page,
            settings.tab,
            settings.scale,
            settings.font,
        ))
    }) else {
        return;
    };
    // The page fills the tab's display area.
    let mut area = RECT::default();
    unsafe {
        let _ = GetClientRect(tabs, &mut area);
        SendMessageW(
            tabs,
            TCM_ADJUSTRECT,
            Some(WPARAM(0)),
            Some(LPARAM(&mut area as *mut _ as isize)),
        );
    }
    let margin = (MARGIN * scale).round() as i32;
    let instance = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    let page = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("TypioSettingsPage"),
            w!(""),
            windows::Win32::UI::WindowsAndMessaging::WS_CHILD
                | windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE
                | WS_CLIPCHILDREN,
            // The tab sits `margin` into the window, the page another `margin` into the tab.
            area.left + 2 * margin,
            area.top + 2 * margin,
            area.right - area.left - 2 * margin,
            area.bottom - area.top - 2 * margin,
            Some(window),
            None,
            Some(instance.into()),
            None,
        )
    }
    .unwrap_or_default();
    unsafe {
        let _ = SetWindowPos(
            page,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
        );
    }
    let width = f64::from(area.right - area.left) / scale;
    let height = f64::from(area.bottom - area.top) / scale;
    let mut builder = Builder {
        page,
        scale,
        font,
        secondary: Vec::new(),
        width,
        height,
    };
    SETTINGS.with_borrow_mut(|slot| {
        if let Some(settings) = slot.as_mut() {
            settings.filling = true;
        }
    });
    match tab {
        0 => general(&mut builder, state),
        1 => datasets(&mut builder, state),
        2 => apps(&mut builder, state),
        _ => extension(&mut builder),
    }
    let secondary = builder.secondary;
    SETTINGS.with_borrow_mut(|slot| {
        if let Some(settings) = slot.as_mut() {
            settings.page = page;
            settings.secondary = secondary;
            settings.shown = Some(state.clone());
            settings.filling = false;
        }
    });
    if !old_page.is_invalid() {
        unsafe {
            let _ = DestroyWindow(old_page);
        }
    }
}

/// Lays out controls on the page, in unscaled pixels.
struct Builder {
    page: HWND,
    scale: f64,
    font: HFONT,
    secondary: Vec<HWND>,
    width: f64,
    height: f64,
}

impl Builder {
    fn add(
        &mut self,
        class: PCWSTR,
        text: &str,
        style: WINDOW_STYLE,
        id: usize,
        frame: (f64, f64, f64, f64),
    ) -> HWND {
        ui::control(
            self.page,
            class,
            text,
            style,
            WINDOW_EX_STYLE(0),
            id,
            frame,
            self.scale,
            self.font,
        )
    }

    fn label(&mut self, text: &str, frame: (f64, f64, f64, f64)) -> HWND {
        self.add(w!("STATIC"), text, STATIC_LEFT, 0, frame)
    }

    fn secondary(&mut self, text: &str, frame: (f64, f64, f64, f64)) -> HWND {
        let label = self.label(text, frame);
        self.secondary.push(label);
        label
    }

    fn button(&mut self, text: &str, id: usize, frame: (f64, f64, f64, f64)) -> HWND {
        self.add(
            w!("BUTTON"),
            text,
            WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
            id,
            frame,
        )
    }

    fn checkbox(&mut self, text: &str, id: usize, checked: bool, frame: (f64, f64, f64, f64)) {
        let checkbox = self.add(
            w!("BUTTON"),
            text,
            WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
            id,
            frame,
        );
        let state = if checked { BST_CHECKED.0 as usize } else { 0 };
        unsafe {
            SendMessageW(checkbox, BM_SETCHECK, Some(WPARAM(state)), None);
        }
    }

    fn combo(&mut self, id: usize, items: &[String], selected: usize, frame: (f64, f64, f64, f64)) {
        let (x, y, width, _) = frame;
        let combo = self.add(
            w!("COMBOBOX"),
            "",
            WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_TABSTOP | WS_VSCROLL,
            id,
            (x, y, width, 240.0),
        );
        for item in items {
            let text = ui::wide(item);
            unsafe {
                SendMessageW(
                    combo,
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(text.as_ptr() as isize)),
                );
            }
        }
        unsafe {
            SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(selected)), None);
        }
    }
}

fn general(page: &mut Builder, state: &UiState) {
    let width = page.width - 2.0 * MARGIN;
    page.checkbox(
        &format!("{}   (Ctrl+Alt+T)", tr(T::EnableTypio)),
        ID_ENABLE,
        state.enabled,
        (0.0, 0.0, width, 24.0),
    );
    page.label(tr(T::TriggerCharacter), (0.0, 44.0, 170.0, 20.0));
    let triggers: Vec<String> = TRIGGERS.iter().map(char::to_string).collect();
    let selected = TRIGGERS
        .iter()
        .position(|trigger| *trigger == state.trigger)
        .unwrap_or(0);
    page.combo(ID_TRIGGER, &triggers, selected, (180.0, 40.0, 80.0, 24.0));
    page.secondary(
        &trf(T::TriggerHint, &[("trigger", &state.trigger.to_string())]),
        (180.0, 72.0, width - 180.0, 20.0),
    );
    page.label(tr(T::LanguageLabel), (0.0, 112.0, 170.0, 20.0));
    let languages: Vec<String> = LANGUAGES.iter().map(|(_, name)| name()).collect();
    let selected = LANGUAGES
        .iter()
        .position(|(language, _)| *language == state.language)
        .unwrap_or(0);
    page.combo(
        ID_LANGUAGE,
        &languages,
        selected,
        (180.0, 108.0, 200.0, 24.0),
    );
}

type LanguageName = fn() -> String;

const LANGUAGES: [(Language, LanguageName); 3] = [
    (Language::System, || tr(T::LanguageSystem).to_string()),
    (Language::English, || "English".to_string()),
    (Language::Vietnamese, || "Tiếng Việt".to_string()),
];

fn datasets(page: &mut Builder, state: &UiState) {
    let width = page.width - 2.0 * MARGIN;
    let buttons_top = page.height - 2.0 * MARGIN - 64.0;
    if state.datasets.is_empty() {
        page.secondary(tr(T::NoDatasetsHelp), (0.0, 0.0, width, 40.0));
    } else {
        page.secondary(tr(T::DatasetsHint), (0.0, 0.0, width, 20.0));
    }
    let list = ui::control(
        page.page,
        WC_LISTVIEWW,
        "",
        WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOCOLUMNHEADER)
            | WS_TABSTOP,
        WS_EX_CLIENTEDGE,
        ID_DATASETS,
        (0.0, 28.0, width, buttons_top - 36.0),
        page.scale,
        page.font,
    );
    let extended = (LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize;
    unsafe {
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(extended as usize)),
            Some(LPARAM(extended)),
        );
    }
    let list_width = ((width - 24.0) * page.scale) as i32;
    for (index, column_width) in [list_width * 2 / 3, list_width / 3].into_iter().enumerate() {
        let column = LVCOLUMNW {
            mask: LVCF_WIDTH,
            cx: column_width,
            ..Default::default()
        };
        unsafe {
            SendMessageW(
                list,
                LVM_INSERTCOLUMNW,
                Some(WPARAM(index)),
                Some(LPARAM(&column as *const _ as isize)),
            );
        }
    }
    for (index, (name, detail, enabled)) in state.datasets.iter().enumerate() {
        let mut name = ui::wide(name);
        let item = LVITEMW {
            mask: LVIF_TEXT,
            iItem: index as i32,
            pszText: windows::core::PWSTR(name.as_mut_ptr()),
            ..Default::default()
        };
        unsafe {
            SendMessageW(
                list,
                LVM_INSERTITEMW,
                None,
                Some(LPARAM(&item as *const _ as isize)),
            );
        }
        let mut detail = ui::wide(detail);
        let sub = LVITEMW {
            iSubItem: 1,
            pszText: windows::core::PWSTR(detail.as_mut_ptr()),
            ..Default::default()
        };
        let check = LVITEMW {
            stateMask: LVIS_STATEIMAGEMASK,
            // State image 2 is checked, 1 unchecked.
            state: windows::Win32::UI::Controls::LIST_VIEW_ITEM_STATE_FLAGS(
                if *enabled { 2 } else { 1 } << 12,
            ),
            ..Default::default()
        };
        unsafe {
            SendMessageW(
                list,
                LVM_SETITEMTEXTW,
                Some(WPARAM(index)),
                Some(LPARAM(&sub as *const _ as isize)),
            );
            SendMessageW(
                list,
                LVM_SETITEMSTATE,
                Some(WPARAM(index)),
                Some(LPARAM(&check as *const _ as isize)),
            );
        }
    }

    let row = |index: f64| index * 104.0;
    let first = buttons_top;
    page.button(tr(T::NewButton), ID_NEW, (row(0.0), first, 96.0, 28.0));
    page.button(tr(T::EditButton), ID_EDIT, (row(1.0), first, 96.0, 28.0));
    page.button(
        tr(T::DeleteButton),
        ID_DELETE,
        (row(2.0), first, 96.0, 28.0),
    );
    let second = first + 36.0;
    page.button(tr(T::ImportButton), ID_IMPORT, (0.0, second, 120.0, 28.0));
    page.button(tr(T::ExportBackup), ID_EXPORT, (128.0, second, 150.0, 28.0));
    page.button(
        tr(T::OpenDataFolder),
        ID_DATA_FOLDER,
        (286.0, second, 170.0, 28.0),
    );
}

fn apps(page: &mut Builder, state: &UiState) {
    let width = page.width - 2.0 * MARGIN;
    let buttons_top = page.height - 2.0 * MARGIN - 28.0;
    page.label(tr(T::RunsIn), (0.0, 4.0, 150.0, 20.0));
    let modes = [tr(T::ModeAll).to_string(), tr(T::ModeSelected).to_string()];
    let selected = usize::from(state.app_mode == AppMode::Selected);
    page.combo(ID_MODE, &modes, selected, (160.0, 0.0, 280.0, 24.0));
    let (about, empty) = match state.app_mode {
        AppMode::All => (T::SkippedAbout, T::SkippedEmpty),
        AppMode::Selected => (T::SelectedAbout, T::SelectedEmpty),
    };
    page.secondary(tr(about), (0.0, 36.0, width, 40.0));
    let list = page.add(
        w!("LISTBOX"),
        "",
        WINDOW_STYLE((LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32)
            | WS_BORDER
            | WS_VSCROLL
            | WS_TABSTOP,
        ID_APPS,
        (0.0, 84.0, width, buttons_top - 92.0),
    );
    if state.apps.is_empty() {
        page.secondary(tr(empty), (0.0, buttons_top - 4.0 - 20.0, width, 20.0));
    }
    for app in &state.apps {
        let label = if app.name.is_empty() {
            app.id.clone()
        } else {
            format!("{}  ({})", app.name, app.id)
        };
        let text = ui::wide(&label);
        unsafe {
            SendMessageW(
                list,
                LB_ADDSTRING,
                None,
                Some(LPARAM(text.as_ptr() as isize)),
            );
        }
    }
    page.button(tr(T::AddApp), ID_ADD_APP, (0.0, buttons_top, 120.0, 28.0));
    page.button(
        tr(T::RemoveButton),
        ID_REMOVE_APP,
        (128.0, buttons_top, 100.0, 28.0),
    );
}

fn extension(page: &mut Builder) {
    let width = page.width - 2.0 * MARGIN;
    page.label(tr(T::ExtensionAbout), (0.0, 0.0, width, 72.0));
    let steps = [
        T::ExtensionStep1,
        T::ExtensionStep2,
        T::ExtensionStep3,
        T::ExtensionStep4,
    ];
    for (index, step) in steps.into_iter().enumerate() {
        let top = 84.0 + index as f64 * 40.0;
        page.label(&format!("{}.", index + 1), (0.0, top, 20.0, 20.0));
        page.label(tr(step), (24.0, top, width - 24.0, 36.0));
    }
    let top = 84.0 + 4.0 * 40.0 + 8.0;
    page.button(tr(T::OpenWebStore), ID_STORE, (0.0, top, 190.0, 28.0));
    page.button(tr(T::OpenWebsite), ID_WEBSITE, (198.0, top, 170.0, 28.0));
    page.button(tr(T::ExportBackup), ID_EXPORT, (376.0, top, 150.0, 28.0));
}

/// The selected row of the list with `id` on the page, if any.
fn selected_row(id: usize) -> Option<usize> {
    let page = SETTINGS.with_borrow(|slot| slot.as_ref().map(|settings| settings.page))?;
    let list =
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(page), id as i32) }
            .ok()?;
    let index = if id == ID_DATASETS {
        unsafe {
            SendMessageW(
                list,
                LVM_GETNEXTITEM,
                Some(WPARAM(usize::MAX)),
                Some(LPARAM(LVNI_SELECTED as isize)),
            )
        }
    } else {
        unsafe { SendMessageW(list, LB_GETCURSEL, None, None) }
    };
    usize::try_from(index.0).ok()
}

fn command(id: usize, code: u32, control: HWND) {
    let owner = settings_window();
    match id {
        ID_ENABLE => {
            let checked = unsafe { SendMessageW(control, BM_GETCHECK, None, None) }.0
                == BST_CHECKED.0 as isize;
            with_app(|app| {
                if app.store.enabled != checked {
                    app.toggle_enabled();
                }
            });
        }
        ID_TRIGGER | ID_LANGUAGE | ID_MODE if code == CBN_SELCHANGE_CODE => {
            let index = unsafe { SendMessageW(control, CB_GETCURSEL, None, None) }.0;
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            match id {
                ID_TRIGGER => {
                    if let Some(trigger) = TRIGGERS.get(index) {
                        with_app(|app| app.set_trigger(*trigger));
                    }
                }
                ID_LANGUAGE => {
                    if let Some((language, _)) = LANGUAGES.get(index) {
                        with_app(|app| app.set_language(*language));
                        // Every text changes: start the window again in the new language.
                        let tab = SETTINGS.with_borrow(|slot| slot.as_ref().map(|s| s.tab));
                        close();
                        show(tab);
                    }
                }
                _ => {
                    let mode = if index == 1 {
                        AppMode::Selected
                    } else {
                        AppMode::All
                    };
                    with_app(|app| app.set_app_mode(mode));
                }
            }
        }
        ID_NEW => editor::open(None),
        ID_EDIT => {
            if let Some(index) = selected_row(ID_DATASETS) {
                editor::open(Some(index));
            }
        }
        ID_DELETE => {
            let Some(index) = selected_row(ID_DATASETS) else {
                return;
            };
            let name = with_app(|app| app.store.datasets.get(index).map(|d| d.name.clone()))
                .flatten()
                .unwrap_or_default();
            if ui::confirm(&trf(T::DeleteTitle, &[("name", &name)]), tr(T::DeleteInfo)) {
                with_app(|app| app.remove_dataset(index));
            }
        }
        ID_IMPORT => ui::import_datasets(owner),
        ID_EXPORT => ui::export_backup(owner),
        ID_DATA_FOLDER => ui::open_data_folder(),
        ID_ADD_APP => {
            let files = ui::choose_files(owner, tr(T::ChooseApps), &[("*.exe", "*.exe")]);
            for file in files {
                let (id, name) = typio_platform::app_for_executable(&file.to_string_lossy());
                if !id.is_empty() {
                    with_app(|app| app.add_app(&id, &name));
                }
            }
        }
        ID_REMOVE_APP => {
            if let Some(index) = selected_row(ID_APPS) {
                with_app(|app| app.remove_app(index));
            }
        }
        ID_STORE => {
            // In Chrome when it is installed, since the extension is for Chrome.
            if !ui::open("chrome.exe", Some(STORE_URL)) {
                ui::open(STORE_URL, None);
            }
        }
        ID_WEBSITE => {
            ui::open(WEBSITE_URL, None);
        }
        _ => {}
    }
}

fn notify(header: &NMHDR, lparam: LPARAM) {
    match (header.idFrom, header.code) {
        (_, code) if code == TCN_SELCHANGE => {
            let tab = unsafe { SendMessageW(header.hwndFrom, TCM_GETCURSEL, None, None) }.0;
            if let Ok(tab) = usize::try_from(tab) {
                select_tab(tab);
            }
        }
        (ID_DATASETS, code) if code == LVN_ITEMCHANGED => {
            let filling = SETTINGS.with_borrow(|slot| slot.as_ref().is_none_or(|s| s.filling));
            if filling {
                return;
            }
            let change = unsafe { &*(lparam.0 as *const NMLISTVIEW) };
            if change.uChanged & LVIF_STATE == LVIF_STATE {
                let old = (change.uOldState >> 12) & 0xf;
                let new = (change.uNewState >> 12) & 0xf;
                if old != 0 && new != 0 && old != new {
                    let index = change.iItem as usize;
                    let enabled = new == 2;
                    with_app(|app| {
                        let id = app.store.datasets.get(index).map(|d| d.id.clone());
                        if let Some(id) = id {
                            if app.store.is_dataset_enabled(&id) != enabled {
                                app.toggle_dataset(index);
                            }
                        }
                    });
                }
            }
        }
        (ID_DATASETS, code) if code == NM_DBLCLK => {
            if let Some(index) = selected_row(ID_DATASETS) {
                editor::open(Some(index));
            }
        }
        _ => {}
    }
}

/// Grey text for hints, the window colour behind every label.
fn color_static(dc: HDC, control: HWND) -> LRESULT {
    let secondary = SETTINGS.with_borrow(|slot| {
        slot.as_ref()
            .is_some_and(|settings| settings.secondary.contains(&control))
    });
    unsafe {
        let color = if secondary {
            GetSysColor(COLOR_GRAYTEXT)
        } else {
            GetSysColor(COLOR_WINDOWTEXT)
        };
        SetTextColor(dc, COLORREF(color));
        SetBkColor(dc, COLORREF(GetSysColor(COLOR_WINDOW)));
        LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize)
    }
}

/// Moves to a screen with another scale: new size, font and layout.
fn rescale(window: HWND, scale: f64, suggested: RECT) {
    let Some((tabs, old_font, font)) = SETTINGS.with_borrow_mut(|slot| {
        let settings = slot.as_mut()?;
        let old_font = settings.font;
        settings.scale = scale;
        settings.font = ui::ui_font(scale);
        settings.shown = None;
        Some((settings.tabs, old_font, settings.font))
    }) else {
        return;
    };
    let margin = (MARGIN * scale).round() as i32;
    unsafe {
        let _ = SetWindowPos(
            window,
            None,
            suggested.left,
            suggested.top,
            suggested.right - suggested.left,
            suggested.bottom - suggested.top,
            SWP_NOZORDER,
        );
        let _ = SetWindowPos(
            tabs,
            None,
            margin,
            margin,
            ((WIDTH - 2.0 * MARGIN) * scale).round() as i32,
            ((HEIGHT - 2.0 * MARGIN) * scale).round() as i32,
            SWP_NOZORDER,
        );
        SendMessageW(
            tabs,
            windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    if let Some(state) = with_app(|app| app.ui_state()) {
        rebuild_page(&state);
    }
    unsafe {
        let _ = DeleteObject(old_font.into());
    }
}

pub fn close() {
    if let Some(window) = settings_window() {
        unsafe {
            let _ = DestroyWindow(window);
        }
    }
}

unsafe extern "system" fn page_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let code = (wparam.0 >> 16) as u32 & 0xffff;
            command(id, code, HWND(lparam.0 as *mut _));
            LRESULT(0)
        }
        WM_NOTIFY => {
            let header = unsafe { &*(lparam.0 as *const NMHDR) };
            notify(header, lparam);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            color_static(HDC(wparam.0 as *mut _), HWND(lparam.0 as *mut _))
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NOTIFY => {
            let header = unsafe { &*(lparam.0 as *const NMHDR) };
            notify(header, lparam);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => color_static(HDC(wparam.0 as *mut _), HWND(lparam.0 as *mut _)),
        WM_DPICHANGED => {
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            let scale = f64::from(wparam.0 as u32 & 0xffff) / 96.0;
            rescale(window, scale, suggested);
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let settings = SETTINGS.with_borrow_mut(Option::take);
            if let Some(settings) = settings {
                unsafe {
                    let _ = DeleteObject(settings.font.into());
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
