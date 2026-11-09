//! Typio for Windows: a tray app with the same suggestions as on macOS. Settings live in
//! the tray menu.

mod controller;
mod popup;

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::time::Duration;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST,
    OPENFILENAMEW,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, KillTimer,
    LoadIconW, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetTimer, TrackPopupMenu, TranslateMessage, HICON, HMENU, IDI_APPLICATION,
    LR_DEFAULTCOLOR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MENU_ITEM_FLAGS,
    MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON, SW_SHOWNORMAL,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_CONTEXTMENU,
    WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

use crate::i18n::{self, tr, trf, Language, T};
use crate::store::{AppMode, Store, TRIGGERS};
use controller::with_app;

const WM_TYPIO_EVENTS: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
pub(crate) const TIMER_REFRESH: usize = 1;
pub(crate) const TIMER_MESSAGE: usize = 2;
pub(crate) const TIMER_DRAIN: usize = 3;

const STORE_URL: &str =
    "https://chromewebstore.google.com/detail/typio/ebndieeiallcbgjbhhkbgnegmfapagjf";
const WEBSITE_URL: &str = "https://typio.thangdv.com/";
const TRAY_ICON: &[u8] = include_bytes!("../../../../assets/tray.png");

// Tray menu commands.
const ID_TOGGLE: usize = 1;
const ID_APP: usize = 2;
const ID_IMPORT: usize = 3;
const ID_DATA_FOLDER: usize = 4;
const ID_STORE: usize = 5;
const ID_WEBSITE: usize = 6;
const ID_QUIT: usize = 7;
const ID_MODE_ALL: usize = 8;
const ID_MODE_SELECTED: usize = 9;
const ID_LANGUAGE: usize = 20;
const ID_TRIGGER: usize = 30;
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
    // One Typio at a time; a second copy quits quietly.
    let _instance = unsafe { CreateMutexW(None, true, w!("Local\\TypioDesktop")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }

    i18n::set(Language::System);
    let store = match Store::load(&Store::data_dir()) {
        Ok(store) => store,
        Err(error) => {
            // Never overwrite data Typio cannot read.
            alert(
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
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TYPIO_EVENTS => controller::drain(),
        WM_TRAY => {
            let event = (lparam.0 & 0xffff) as u32;
            if event == WM_LBUTTONUP || event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                show_menu(window);
            }
        }
        WM_TIMER => {
            let id = wparam.0;
            unsafe {
                let _ = KillTimer(Some(window), id);
            }
            let done = match id {
                TIMER_REFRESH => with_app(|app| app.refresh()).is_some(),
                TIMER_MESSAGE => with_app(|app| app.message_expired()).is_some(),
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

/// Adds, updates or removes the tray icon; its tooltip says whether Typio is on.
fn tray(action: windows::Win32::UI::Shell::NOTIFY_ICON_MESSAGE, enabled: bool) {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: main_window(),
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: tray_icon(),
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

pub(crate) fn update_tray(enabled: bool) {
    tray(NIM_MODIFY, enabled);
}

/// The Typio logo at the tray's icon size, made once.
fn tray_icon() -> HICON {
    thread_local! {
        static ICON: std::cell::OnceCell<HICON> = const { std::cell::OnceCell::new() };
    }
    ICON.with(|icon| {
        *icon.get_or_init(|| {
            let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
            unsafe {
                CreateIconFromResourceEx(TRAY_ICON, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
            }
            .or_else(|_| unsafe { LoadIconW(None, IDI_APPLICATION) })
            .unwrap_or_default()
        })
    })
}

/// Builds the tray menu from the current state, shows it and runs the chosen command.
fn show_menu(window: HWND) {
    let Some(state) = with_app(|app| app.menu_state()) else {
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
    append(
        menu,
        MENU_ITEM_FLAGS(0),
        ID_DATA_FOLDER,
        tr(T::OpenDataFolder),
    );
    separator(menu);

    if let Ok(triggers) = unsafe { CreatePopupMenu() } {
        for (index, trigger) in TRIGGERS.iter().enumerate() {
            append(
                triggers,
                checked(*trigger == state.trigger),
                ID_TRIGGER + index,
                &trigger.to_string(),
            );
        }
        submenu(menu, triggers, tr(T::TriggerCharacter));
    }
    if let Ok(modes) = unsafe { CreatePopupMenu() } {
        append(
            modes,
            checked(state.app_mode == AppMode::All),
            ID_MODE_ALL,
            tr(T::ModeAll),
        );
        append(
            modes,
            checked(state.app_mode == AppMode::Selected),
            ID_MODE_SELECTED,
            tr(T::ModeSelected),
        );
        submenu(menu, modes, tr(T::RunsIn));
    }
    if let Ok(languages) = unsafe { CreatePopupMenu() } {
        for (index, (language, label)) in languages_list().iter().enumerate() {
            append(
                languages,
                checked(*language == state.language),
                ID_LANGUAGE + index,
                label,
            );
        }
        submenu(menu, languages, tr(T::LanguageLabel));
    }
    if let Ok(extension) = unsafe { CreatePopupMenu() } {
        append(extension, MENU_ITEM_FLAGS(0), ID_STORE, tr(T::OpenWebStore));
        append(
            extension,
            MENU_ITEM_FLAGS(0),
            ID_WEBSITE,
            tr(T::OpenWebsite),
        );
        submenu(
            menu,
            extension,
            tr(T::InstallExtension).trim_end_matches('…'),
        );
    }
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

fn languages_list() -> [(Language, &'static str); 3] {
    [
        (Language::System, tr(T::LanguageSystem)),
        (Language::English, "English"),
        (Language::Vietnamese, "Tiếng Việt"),
    ]
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
        ID_IMPORT => import_datasets(window),
        ID_DATA_FOLDER => {
            let dir = Store::data_dir();
            let _ = std::fs::create_dir_all(&dir);
            open(&dir.to_string_lossy(), None);
        }
        ID_STORE => {
            // In Chrome when it is installed, since the extension is for Chrome.
            if !open("chrome.exe", Some(STORE_URL)) {
                open(STORE_URL, None);
            }
        }
        ID_WEBSITE => {
            open(WEBSITE_URL, None);
        }
        ID_MODE_ALL => {
            with_app(|app| app.set_app_mode(AppMode::All));
        }
        ID_MODE_SELECTED => {
            with_app(|app| app.set_app_mode(AppMode::Selected));
        }
        ID_QUIT => unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(window);
        },
        _ if (ID_LANGUAGE..ID_LANGUAGE + 3).contains(&command) => {
            let (language, _) = languages_list()[command - ID_LANGUAGE];
            with_app(|app| app.set_language(language));
            update_tray(with_app(|app| app.store.enabled).unwrap_or(true));
        }
        _ if (ID_TRIGGER..ID_TRIGGER + TRIGGERS.len()).contains(&command) => {
            let trigger = TRIGGERS[command - ID_TRIGGER];
            with_app(|app| app.set_trigger(trigger));
        }
        _ if command >= ID_DATASET => {
            with_app(|app| app.toggle_dataset(command - ID_DATASET));
        }
        _ => {}
    }
}

fn append(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    let text = wide(text);
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING | flags, id, PCWSTR(text.as_ptr()));
    }
}

fn separator(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    }
}

fn submenu(menu: HMENU, child: HMENU, text: &str) {
    let text = wide(text);
    unsafe {
        let _ = AppendMenuW(
            menu,
            MF_STRING | MF_POPUP,
            child.0 as usize,
            PCWSTR(text.as_ptr()),
        );
    }
}

/// Opens a file, folder or link with its default app, or runs `file` with `parameters`.
/// Returns whether Windows started something.
fn open(file: &str, parameters: Option<&str>) -> bool {
    let file = wide(file);
    let parameters = parameters.map(wide);
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(file.as_ptr()),
            parameters
                .as_ref()
                .map_or(PCWSTR::null(), |parameters| PCWSTR(parameters.as_ptr())),
            None,
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success.
    result.0 as isize > 32
}

fn import_datasets(window: HWND) {
    let mut buffer = vec![0u16; 32 * 1024];
    let filter: Vec<u16> = "Typio data (*.txt, *.csv, *.json)\0*.txt;*.csv;*.json\0*.*\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title = wide(tr(T::ChooseImportFiles));
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: window,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_ALLOWMULTISELECT | OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
        return;
    }
    let mut messages = Vec::new();
    for path in chosen_files(&buffer) {
        let result = with_app(|app| {
            let result = app.store.import(&path);
            if result.is_ok() {
                app.store_changed();
            }
            result
        });
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        messages.push(match result {
            Some(Ok(message)) => message,
            Some(Err(error)) => format!("{name}: {error}"),
            None => trf(T::ImportBusy, &[("name", &name)]),
        });
    }
    alert(tr(T::ImportFinished), &messages.join("\n"));
}

/// The open dialog returns one full path, or a folder followed by file names, each ended
/// by a NUL and the list by two.
fn chosen_files(buffer: &[u16]) -> Vec<PathBuf> {
    let parts: Vec<String> = buffer
        .split(|unit| *unit == 0)
        .take_while(|part| !part.is_empty())
        .map(String::from_utf16_lossy)
        .collect();
    match parts.as_slice() {
        [] => Vec::new(),
        [single] => vec![PathBuf::from(single)],
        [folder, files @ ..] => files
            .iter()
            .map(|file| PathBuf::from(folder).join(file))
            .collect(),
    }
}

fn alert(title: &str, text: &str) {
    let title = wide(title);
    let text = wide(text);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::chosen_files;
    use std::path::PathBuf;

    fn buffer(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn reads_one_or_many_chosen_files() {
        assert_eq!(
            chosen_files(&buffer("C:\\data\\a.txt\0\0")),
            vec![PathBuf::from("C:\\data\\a.txt")]
        );
        assert_eq!(
            chosen_files(&buffer("C:\\data\0a.txt\0b.csv\0\0")),
            vec![
                PathBuf::from("C:\\data").join("a.txt"),
                PathBuf::from("C:\\data").join("b.csv")
            ]
        );
    }
}
