//! Typio for Windows: a tray app with the same suggestions as on macOS, a tray menu and a
//! Settings window.

mod controller;
mod editor;
mod popup;
mod settings;
mod ui;

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, ICC_TAB_CLASSES,
    INITCOMMONCONTROLSEX,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, NOTIFY_ICON_MESSAGE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW,
    GetSystemMetrics, IsChild, IsDialogMessageW, KillTimer, LoadIconW, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, SetTimer,
    TrackPopupMenu, TranslateMessage, HICON, HMENU, IDI_APPLICATION, LR_DEFAULTCOLOR,
    MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_CONTEXTMENU,
    WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSW, WS_OVERLAPPED,
};

use crate::i18n::{self, tr, trf, Language, T};
use crate::store::{AppMode, Store};
use controller::with_app;

const WM_TYPIO_EVENTS: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
const WM_TYPIO_UI: u32 = WM_APP + 3;
/// Sent by a second copy of Typio: show Settings, so starting Typio again shows something.
const WM_TYPIO_SHOW: u32 = WM_APP + 4;
pub(crate) const TIMER_REFRESH: usize = 1;
pub(crate) const TIMER_MESSAGE: usize = 2;
pub(crate) const TIMER_DRAIN: usize = 3;
const TIMER_UI: usize = 4;

const TRAY_ICON: &[u8] = include_bytes!("../../../../assets/tray.png");
const TRAY_ICON_OFF: &[u8] = include_bytes!("../../../../assets/tray-off.png");

// Tray menu commands.
const ID_TOGGLE: usize = 1;
const ID_APP: usize = 2;
const ID_IMPORT: usize = 3;
const ID_EXTENSION: usize = 4;
const ID_SETTINGS: usize = 5;
const ID_QUIT: usize = 6;
const ID_DATASET: usize = 100;

/// The hidden main window, which gets the tray and hook messages.
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn main_window() -> HWND {
    HWND(MAIN_WINDOW.load(Ordering::Relaxed) as *mut _)
}

/// Tells the main thread that commands are queued; safe from any thread.
pub(crate) fn wake() {
    unsafe {
        let _ = PostMessageW(Some(main_window()), WM_TYPIO_EVENTS, WPARAM(0), LPARAM(0));
    }
}

/// Brings the tray icon and the Settings window up to date once the current work is done.
pub(crate) fn refresh_ui() {
    unsafe {
        let _ = PostMessageW(Some(main_window()), WM_TYPIO_UI, WPARAM(0), LPARAM(0));
    }
}

/// Runs `id`'s timer once after `delay`, replacing an earlier one with the same id.
pub(crate) fn set_timer(id: usize, delay: Duration) {
    unsafe {
        SetTimer(Some(main_window()), id, delay.as_millis() as u32, None);
    }
}

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    // One Typio at a time; starting it again opens the running copy's Settings.
    let _instance = unsafe { CreateMutexW(None, true, w!("Local\\TypioDesktop")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if let Ok(running) = unsafe { FindWindowW(w!("TypioMain"), None) } {
            unsafe {
                let _ = PostMessageW(Some(running), WM_TYPIO_SHOW, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }
    let controls = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_STANDARD_CLASSES | ICC_LISTVIEW_CLASSES | ICC_TAB_CLASSES,
    };
    unsafe {
        let _ = InitCommonControlsEx(&controls);
    }

    i18n::set(Language::System);
    let store = match Store::load(&Store::data_dir()) {
        Ok(store) => store,
        Err(error) => {
            // Never overwrite data Typio cannot read.
            ui::alert(
                tr(T::CannotReadData),
                &trf(
                    T::DataFileIn,
                    &[
                        ("error", &error.to_string()),
                        ("path", &Store::data_dir().display().to_string()),
                    ],
                ),
            );
            return;
        }
    };
    i18n::set(store.language);
    let enabled = store.enabled;

    let instance = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: w!("TypioMain"),
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("TypioMain"),
            w!("Typio"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    };
    let Ok(window) = window else {
        info!("Could not create the main window");
        return;
    };
    MAIN_WINDOW.store(window.0 as isize, Ordering::Relaxed);
    TASKBAR_CREATED.store(
        unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        Ordering::Relaxed,
    );

    controller::install(store);
    tray(NIM_ADD, enabled);
    with_app(|app| app.start_tap());
    let executable = std::env::current_exe().unwrap_or_default();
    info!(
        "Typio {} started from {}",
        env!("CARGO_PKG_VERSION"),
        executable.display()
    );

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        if handled_by_windows(&message) {
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Ctrl+S in the editor, and Tab, Enter and Escape in the Settings and editor windows.
fn handled_by_windows(message: &MSG) -> bool {
    let belongs_to =
        |window: HWND| message.hwnd == window || unsafe { IsChild(window, message.hwnd) }.as_bool();
    if let Some(editor) = editor::window() {
        if belongs_to(editor) {
            let control = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0;
            if message.message == WM_KEYDOWN && message.wParam.0 == usize::from(b'S') && control {
                editor::save();
                return true;
            }
            return unsafe { IsDialogMessageW(editor, message) }.as_bool();
        }
    }
    if let Some(settings) = settings::window() {
        if belongs_to(settings) {
            return unsafe { IsDialogMessageW(settings, message) }.as_bool();
        }
    }
    false
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TYPIO_EVENTS => controller::drain(),
        WM_TYPIO_UI => update_ui(),
        WM_TYPIO_SHOW => settings::show(None),
        WM_TRAY => match (lparam.0 & 0xffff) as u32 {
            WM_LBUTTONDBLCLK => settings::show(None),
            WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(window),
            _ => {}
        },
        WM_TIMER => {
            let id = wparam.0;
            unsafe {
                let _ = KillTimer(Some(window), id);
            }
            let done = match id {
                TIMER_REFRESH => with_app(|app| app.refresh()).is_some(),
                TIMER_MESSAGE => with_app(|app| app.message_expired()).is_some(),
                TIMER_UI => {
                    update_ui();
                    true
                }
                _ => {
                    controller::drain();
                    true
                }
            };
            // Busy (a UI Automation call let messages through): try again shortly.
            if !done {
                set_timer(id, Duration::from_millis(10));
            }
        }
        WM_DESTROY => {
            tray(NIM_DELETE, false);
            unsafe { PostQuitMessage(0) };
        }
        _ if message == TASKBAR_CREATED.load(Ordering::Relaxed) && message != 0 => {
            // Explorer restarted: put the icon back.
            let enabled = with_app(|app| app.store.enabled).unwrap_or(true);
            tray(NIM_ADD, enabled);
        }
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    LRESULT(0)
}

fn update_ui() {
    match with_app(|app| app.ui_state()) {
        Some(state) => {
            tray(NIM_MODIFY, state.enabled);
            settings::update(&state);
        }
        None => set_timer(TIMER_UI, Duration::from_millis(10)),
    }
}

/// Adds, updates or removes the tray icon; the icon and its tooltip say whether Typio is on.
fn tray(action: NOTIFY_ICON_MESSAGE, enabled: bool) {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: main_window(),
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: tray_icon(enabled),
        ..Default::default()
    };
    let tip = if enabled {
        tr(T::StatusOn)
    } else {
        tr(T::StatusOff)
    };
    for (slot, unit) in data.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
        *slot = unit;
    }
    unsafe {
        let _ = Shell_NotifyIconW(action, &data);
    }
}

/// The Typio logo at the tray's icon size, in colour while Typio is on and grey while it is
/// off; each made once.
fn tray_icon(enabled: bool) -> HICON {
    thread_local! {
        static ICONS: std::cell::OnceCell<(HICON, HICON)> = const { std::cell::OnceCell::new() };
    }
    let make = |png: &[u8]| {
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
        unsafe { CreateIconFromResourceEx(png, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) }
            .or_else(|_| unsafe { LoadIconW(None, IDI_APPLICATION) })
            .unwrap_or_default()
    };
    ICONS.with(|icons| {
        let (on, off) = *icons.get_or_init(|| (make(TRAY_ICON), make(TRAY_ICON_OFF)));
        if enabled {
            on
        } else {
            off
        }
    })
}

/// Builds the tray menu from the current state, shows it and runs the chosen command.
fn show_menu(window: HWND) {
    let Some(state) = with_app(|app| app.ui_state()) else {
        return;
    };
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    let checked = |on: bool| if on { MF_CHECKED } else { MENU_ITEM_FLAGS(0) };
    let status = if state.enabled {
        tr(T::StatusOn)
    } else {
        tr(T::StatusOff)
    };
    append(menu, MF_GRAYED, 0, status);
    append(
        menu,
        checked(state.enabled),
        ID_TOGGLE,
        &format!("{}\tCtrl+Alt+T", tr(T::EnableTypio)),
    );
    if let Some((name, listed)) = &state.app {
        let label = match (state.app_mode, listed) {
            (AppMode::All, false) => trf(T::SkipApp, &[("name", name)]),
            (AppMode::All, true) => trf(T::StopSkippingApp, &[("name", name)]),
            (AppMode::Selected, false) => trf(T::RunInApp, &[("name", name)]),
            (AppMode::Selected, true) => trf(T::StopRunningInApp, &[("name", name)]),
        };
        append(menu, MENU_ITEM_FLAGS(0), ID_APP, &label);
    }
    separator(menu);

    if state.datasets.is_empty() {
        append(menu, MF_GRAYED, 0, tr(T::NoDatasets));
    } else {
        append(menu, MF_GRAYED, 0, tr(T::TabDatasets));
        for (index, (name, detail, enabled)) in state.datasets.iter().enumerate() {
            append(
                menu,
                checked(*enabled),
                ID_DATASET + index,
                &format!("{name}\t{detail}"),
            );
        }
    }
    append(menu, MENU_ITEM_FLAGS(0), ID_IMPORT, tr(T::ImportDataset));
    separator(menu);
    append(
        menu,
        MENU_ITEM_FLAGS(0),
        ID_EXTENSION,
        tr(T::InstallExtension),
    );
    append(menu, MENU_ITEM_FLAGS(0), ID_SETTINGS, tr(T::SettingsItem));
    separator(menu);
    append(menu, MENU_ITEM_FLAGS(0), ID_QUIT, tr(T::Quit));

    let mut cursor = POINT::default();
    let command = unsafe {
        let _ = GetCursorPos(&mut cursor);
        // Without this the menu stays open after clicking elsewhere.
        let _ = SetForegroundWindow(window);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            cursor.x,
            cursor.y,
            None,
            window,
            None,
        );
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    };
    run_command(window, command);
}

fn run_command(window: HWND, command: usize) {
    match command {
        0 => {}
        ID_TOGGLE => {
            with_app(|app| app.toggle_enabled());
        }
        ID_APP => {
            with_app(|app| app.toggle_last_app());
        }
        ID_IMPORT => ui::import_datasets(None),
        ID_EXTENSION => settings::show(Some(settings::EXTENSION_TAB)),
        ID_SETTINGS => settings::show(None),
        ID_QUIT => {
            editor::close();
            settings::close();
            unsafe {
                let _ = DestroyWindow(window);
            }
        }
        _ if command >= ID_DATASET => {
            with_app(|app| app.toggle_dataset(command - ID_DATASET));
        }
        _ => {}
    }
}

fn append(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    let text = ui::wide(text);
    unsafe {
        let _ = AppendMenuW(
            menu,
            MF_STRING | flags,
            id,
            windows::core::PCWSTR(text.as_ptr()),
        );
    }
}

fn separator(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    }
}
