//! Small helpers for the Win32 windows: text, fonts, controls and dialogs.

use std::path::PathBuf;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, FW_NORMAL, HFONT,
    OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST,
    OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetWindowTextLengthW, GetWindowTextW, MessageBoxW, SendMessageW, HMENU, IDYES,
    MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_YESNO, SW_SHOWNORMAL,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT, WS_CHILD, WS_VISIBLE,
};

use crate::i18n::{tr, T};
use crate::store::Store;

/// The Typio icon from Typio.exe's resources (`MAKEINTRESOURCE(1)`), or none when the exe
/// has no icon (a build made without the resource compiler).
pub fn app_icon() -> windows::Win32::UI::WindowsAndMessaging::HICON {
    let instance = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    // A resource id goes where the name pointer would.
    #[allow(clippy::manual_dangling_ptr)]
    let id = PCWSTR(1 as *const u16);
    unsafe { windows::Win32::UI::WindowsAndMessaging::LoadIconW(Some(instance.into()), id) }
        .unwrap_or_default()
}

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Segoe UI at `points` for the given DPI scale.
pub fn font(points: f64, scale: f64, face: PCWSTR) -> HFONT {
    unsafe {
        CreateFontW(
            -((points * 96.0 / 72.0 * scale).round() as i32),
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
            face,
        )
    }
}

pub fn ui_font(scale: f64) -> HFONT {
    font(9.0, scale, w!("Segoe UI"))
}

/// A child control of `class` at (`x`, `y`, `width`, `height`) in unscaled pixels.
#[allow(clippy::too_many_arguments)]
pub fn control(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    ex_style: WINDOW_EX_STYLE,
    id: usize,
    frame: (f64, f64, f64, f64),
    scale: f64,
    font: HFONT,
) -> HWND {
    let text = wide(text);
    let (x, y, width, height) = frame;
    let instance = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    let window = unsafe {
        CreateWindowExW(
            ex_style,
            class,
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            (x * scale).round() as i32,
            (y * scale).round() as i32,
            (width * scale).round() as i32,
            (height * scale).round() as i32,
            Some(parent),
            Some(HMENU(id as *mut _)),
            Some(instance.into()),
            None,
        )
    }
    .unwrap_or_default();
    unsafe {
        SendMessageW(
            window,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    window
}

pub fn window_text(window: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(window) };
    let mut buffer = vec![0u16; length as usize + 1];
    let read = unsafe { GetWindowTextW(window, &mut buffer) };
    String::from_utf16_lossy(&buffer[..read as usize])
}

pub fn alert(title: &str, text: &str) {
    message_box(title, text, MB_OK | MB_ICONINFORMATION);
}

/// Asks a yes/no question; true for yes.
pub fn confirm(title: &str, text: &str) -> bool {
    message_box(title, text, MB_YESNO | MB_ICONWARNING) == IDYES.0
}

fn message_box(
    title: &str,
    text: &str,
    style: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE,
) -> i32 {
    let title = wide(title);
    let text = wide(text);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            style | MB_SETFOREGROUND,
        )
        .0
    }
}

/// Opens a file, folder or link with its default app, or runs `file` with `parameters`.
/// Returns whether Windows started something.
pub fn open(file: &str, parameters: Option<&str>) -> bool {
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

pub fn open_data_folder() {
    let dir = Store::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    open(&dir.to_string_lossy(), None);
}

/// Asks for files to open; `filter` is pairs of description and pattern.
pub fn choose_files(owner: Option<HWND>, title: &str, filter: &[(&str, &str)]) -> Vec<PathBuf> {
    let mut buffer = vec![0u16; 32 * 1024];
    let filter = filter_text(filter);
    let title = wide(title);
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner.unwrap_or_default(),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_ALLOWMULTISELECT | OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
        return Vec::new();
    }
    chosen_files(&buffer)
}

/// Asks where to save a file, starting from `name`.
pub fn choose_save_path(
    owner: Option<HWND>,
    name: &str,
    filter: &[(&str, &str)],
) -> Option<PathBuf> {
    let mut buffer = vec![0u16; 4096];
    for (slot, unit) in buffer.iter_mut().zip(name.encode_utf16()) {
        *slot = unit;
    }
    let filter = filter_text(filter);
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner.unwrap_or_default(),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrDefExt: w!("json"),
        Flags: OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetSaveFileNameW(&mut dialog) }.as_bool() {
        return None;
    }
    chosen_files(&buffer).into_iter().next()
}

fn filter_text(filter: &[(&str, &str)]) -> Vec<u16> {
    let mut text = String::new();
    for (description, pattern) in filter {
        text.push_str(description);
        text.push('\0');
        text.push_str(pattern);
        text.push('\0');
    }
    text.push('\0');
    text.encode_utf16().collect()
}

/// The open dialog returns one full path, or a folder followed by file names, each ended
/// by a NUL and the list by two.
pub fn chosen_files(buffer: &[u16]) -> Vec<PathBuf> {
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

/// Imports TXT, CSV or JSON files the user picks, then lists what happened.
pub fn import_datasets(owner: Option<HWND>) {
    let files = choose_files(
        owner,
        tr(T::ChooseImportFiles),
        &[("TXT, CSV, JSON", "*.txt;*.csv;*.json"), ("*.*", "*.*")],
    );
    if files.is_empty() {
        return;
    }
    let mut messages = Vec::new();
    for path in files {
        let result = super::controller::with_app(|app| {
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
            None => crate::i18n::trf(T::ImportBusy, &[("name", &name)]),
        });
    }
    alert(tr(T::ImportFinished), &messages.join("\n"));
}

/// Saves every dataset in the extension's backup format where the user chooses.
pub fn export_backup(owner: Option<HWND>) {
    let Some(backup) = super::controller::with_app(|app| app.store.backup()) else {
        return;
    };
    let date = humantime::format_rfc3339(std::time::SystemTime::now()).to_string();
    let name = format!("typio-datasets-{}.json", &date[..10]);
    let Some(path) = choose_save_path(owner, &name, &[("JSON", "*.json")]) else {
        return;
    };
    if let Err(error) = std::fs::write(&path, backup) {
        alert(tr(T::BackupFailed), &error.to_string());
    }
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
