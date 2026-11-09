//! The focused text field through UI Automation, and typing into it with `SendInput`.

use std::cell::OnceCell;
use std::ffi::c_void;

use windows::core::{BSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, SAFEARRAY,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetUBound, SafeArrayUnaccessData,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextRange,
    IUIAutomationValuePattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
    TextUnit_Character, UIA_ComboBoxControlTypeId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
};

use super::SYNTHETIC_MARK;
use crate::{FieldInfo, Rect};

/// Longest text read from a field, in UTF-16 units; enough for any form field.
const MAX_TEXT: i32 = 1_000_000;

thread_local! {
    static AUTOMATION: OnceCell<Option<IUIAutomation>> = const { OnceCell::new() };
}

/// A focused text field. Text goes in as keystrokes to the window in front.
#[derive(Debug, Clone)]
pub struct Field {
    pub info: FieldInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertError {
    /// The caret moved, another window came to the front, or Windows refused the input.
    Unsupported,
}

/// Why there is no field to work with; logged to diagnose apps that hide their fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusError {
    /// UI Automation could not start or reports no focused element. `code` is the HRESULT.
    NoFocusedElement { app_id: Option<String>, code: i32 },
    /// Focus is on something that is not a text field.
    NotEditable {
        app_id: String,
        role: String,
        subrole: String,
    },
    /// The focused element does not say which process owns it.
    NoProcess,
    /// Typio's own window is in front; Typio never reads itself.
    OwnApp,
}

fn automation() -> Option<IUIAutomation> {
    AUTOMATION.with(|cell| {
        cell.get_or_init(|| unsafe {
            // Already initialized on this thread is fine too.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
        })
        .clone()
    })
}

/// The text field with keyboard focus.
pub fn focused_field() -> Result<Field, FocusError> {
    let front_pid = foreground_pid();
    if front_pid == Some(std::process::id()) {
        return Err(FocusError::OwnApp);
    }
    let no_element = |code: i32| FocusError::NoFocusedElement {
        app_id: front_pid.map(|pid| app_identity(pid).0),
        code,
    };
    let automation = automation().ok_or_else(|| no_element(0))?;
    let element =
        unsafe { automation.GetFocusedElement() }.map_err(|error| no_element(error.code().0))?;

    let pid = unsafe { element.CurrentProcessId() }
        .ok()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid != 0)
        .ok_or(FocusError::NoProcess)?;
    if pid == std::process::id() {
        return Err(FocusError::OwnApp);
    }
    let control = unsafe { element.CurrentControlType() }.unwrap_or_default();
    let role = if control == UIA_EditControlTypeId {
        "Edit".to_string()
    } else if control == UIA_DocumentControlTypeId {
        "Document".to_string()
    } else if control == UIA_ComboBoxControlTypeId {
        "ComboBox".to_string()
    } else {
        format!("ControlType {}", control.0)
    };
    let secure = unsafe { element.CurrentIsPassword() }.is_ok_and(|value| value.as_bool());
    let text_pattern: Option<IUIAutomationTextPattern> =
        unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }.ok();
    let value_pattern: Option<IUIAutomationValuePattern> =
        unsafe { element.GetCurrentPatternAs(UIA_ValuePatternId) }.ok();
    let writable_value = value_pattern.as_ref().is_some_and(|pattern| {
        unsafe { pattern.CurrentIsReadOnly() }.is_ok_and(|ro| !ro.as_bool())
    });
    let text_control = control == UIA_EditControlTypeId || control == UIA_DocumentControlTypeId;
    if !secure && !writable_value && !(text_control && text_pattern.is_some()) {
        return Err(FocusError::NotEditable {
            app_id: app_identity(pid).0,
            role,
            subrole: String::new(),
        });
    }

    let (app_id, app_name) = app_identity(pid);
    let text = |value: windows::core::Result<BSTR>| value.map(|text| text.to_string()).ok();
    let label = [
        text(unsafe { element.CurrentName() }),
        text(unsafe { element.CurrentHelpText() }),
    ]
    .into_iter()
    .flatten()
    .filter(|text| !text.is_empty())
    .collect::<Vec<_>>()
    .join(" ");

    let mut info = FieldInfo {
        app_id,
        app_name,
        pid: pid as i32,
        role,
        identifier: text(unsafe { element.CurrentAutomationId() }).unwrap_or_default(),
        label,
        secure,
        field_rect: unsafe { element.CurrentBoundingRectangle() }
            .ok()
            .map(rect_from),
        ..FieldInfo::default()
    };
    // Password fields are never read.
    if !secure {
        match &text_pattern {
            Some(pattern) => read_text(pattern, &mut info),
            None => {
                info.value = value_pattern
                    .as_ref()
                    .and_then(|pattern| text(unsafe { pattern.CurrentValue() }))
                    .unwrap_or_default();
            }
        }
        if info.caret.is_some() && info.caret_rect.is_none() {
            info.caret_rect = system_caret_rect();
        }
    }
    Ok(Field { info })
}

/// Reads the text, the caret offset and the caret rectangle through the text pattern.
fn read_text(pattern: &IUIAutomationTextPattern, info: &mut FieldInfo) {
    let Ok(document) = (unsafe { pattern.DocumentRange() }) else {
        return;
    };
    info.value = unsafe { document.GetText(MAX_TEXT) }
        .map(|text| text.to_string())
        .unwrap_or_default();
    let Some(selection) = (unsafe { pattern.GetSelection() })
        .ok()
        .filter(|ranges| unsafe { ranges.Length() }.is_ok_and(|length| length == 1))
        .and_then(|ranges| unsafe { ranges.GetElement(0) }.ok())
    else {
        return;
    };
    // Only a caret, not a selection.
    let collapsed = unsafe {
        selection.CompareEndpoints(
            TextPatternRangeEndpoint_Start,
            &selection,
            TextPatternRangeEndpoint_End,
        )
    };
    if collapsed != Ok(0) {
        return;
    }
    let Ok(before) = (unsafe { document.Clone() }) else {
        return;
    };
    if unsafe {
        before.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            &selection,
            TextPatternRangeEndpoint_Start,
        )
    }
    .is_err()
    {
        return;
    }
    let Ok(text_before) = (unsafe { before.GetText(MAX_TEXT) }) else {
        return;
    };
    info.caret = Some(text_before.to_string().encode_utf16().count());
    info.caret_rect = caret_rect(&selection);
}

fn caret_rect(caret: &IUIAutomationTextRange) -> Option<Rect> {
    let usable = |rect: &Rect| rect.height > 0.0;
    if let Some(rect) = bounding_rect(caret).filter(usable) {
        return Some(Rect { width: 1.0, ..rect });
    }
    // Many controls give no rectangle for an empty range; use the character before it.
    let previous = unsafe { caret.Clone() }.ok()?;
    let moved = unsafe {
        previous.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -1)
    };
    if moved != Ok(-1) {
        return None;
    }
    bounding_rect(&previous).filter(usable).map(|rect| Rect {
        x: rect.x + rect.width,
        width: 1.0,
        ..rect
    })
}

/// The first rectangle UI Automation reports for `range`.
fn bounding_rect(range: &IUIAutomationTextRange) -> Option<Rect> {
    let array: *mut SAFEARRAY = unsafe { range.GetBoundingRectangles() }.ok()?;
    if array.is_null() {
        return None;
    }
    let mut result = None;
    unsafe {
        // Left, top, width and height for each line, as doubles.
        if SafeArrayGetUBound(array, 1).is_ok_and(|upper| upper >= 3) {
            let mut data: *mut c_void = std::ptr::null_mut();
            if SafeArrayAccessData(array, &mut data).is_ok() {
                let values = std::slice::from_raw_parts(data as *const f64, 4);
                result = Some(Rect {
                    x: values[0],
                    y: values[1],
                    width: values[2],
                    height: values[3],
                });
                let _ = SafeArrayUnaccessData(array);
            }
        }
        let _ = SafeArrayDestroy(array);
    }
    result
}

/// The caret of the window in front, for classic Win32 edit controls.
fn system_caret_rect() -> Option<Rect> {
    let window = unsafe { GetForegroundWindow() };
    let thread = unsafe { GetWindowThreadProcessId(window, None) };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread, &mut info) }.ok()?;
    if info.hwndCaret.is_invalid() {
        return None;
    }
    let caret = info.rcCaret;
    let mut origin = POINT {
        x: caret.left,
        y: caret.top,
    };
    if !unsafe { ClientToScreen(info.hwndCaret, &mut origin) }.as_bool() {
        return None;
    }
    Some(Rect {
        x: f64::from(origin.x),
        y: f64::from(origin.y),
        width: 1.0,
        height: f64::from(caret.bottom - caret.top),
    })
}

impl Field {
    /// Replaces `length` UTF-16 units at `start` with `text` by typing: Backspaces for the
    /// old text, then the new text. The caret must be right after the old text.
    pub fn replace(&self, start: usize, length: usize, text: &str) -> Result<(), InsertError> {
        if self.info.caret != Some(start + length)
            || foreground_pid() != u32::try_from(self.info.pid).ok()
        {
            return Err(InsertError::Unsupported);
        }
        let removed = utf16_slice(&self.info.value, start, start + length);
        let mut inputs = Vec::new();
        for _ in removed.chars() {
            push_key(&mut inputs, VK_BACK);
        }
        for character in text.chars() {
            match character {
                '\r' => {}
                '\n' => push_key(&mut inputs, VK_RETURN),
                _ => {
                    let mut units = [0; 2];
                    for unit in character.encode_utf16(&mut units) {
                        for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
                            inputs.push(keyboard_input(VIRTUAL_KEY(0), *unit, flags));
                        }
                    }
                }
            }
        }
        send(&inputs)
    }
}

/// Presses Backspace in the window in front, for a Backspace Typio held back.
pub fn press_backspace() -> Result<(), InsertError> {
    let mut inputs = Vec::new();
    push_key(&mut inputs, VK_BACK);
    send(&inputs)
}

fn push_key(inputs: &mut Vec<INPUT>, key: VIRTUAL_KEY) {
    inputs.push(keyboard_input(key, 0, KEYBD_EVENT_FLAGS(0)));
    inputs.push(keyboard_input(key, 0, KEYEVENTF_KEYUP));
}

fn keyboard_input(key: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SYNTHETIC_MARK,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), InsertError> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(InsertError::Unsupported)
    }
}

/// The executable name (lowercase, such as `notepad.exe`) and display name of the app in
/// front.
pub fn frontmost_app() -> Option<(String, String)> {
    let pid = foreground_pid()?;
    (pid != std::process::id()).then(|| app_identity(pid))
}

fn foreground_pid() -> Option<u32> {
    let window: HWND = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

fn app_identity(pid: u32) -> (String, String) {
    match process_path(pid) {
        Some(path) => app_for_executable(&path),
        None => (format!("pid:{pid}"), String::new()),
    }
}

/// The app id (lowercase executable name, such as `notepad.exe`) and display name for the
/// executable at `path`, as Typio lists apps.
pub fn app_for_executable(path: &str) -> (String, String) {
    let file = std::path::Path::new(path);
    let id = file
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let name = file_description(path).unwrap_or_else(|| {
        file.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    (id, name)
}

fn process_path(pid: u32) -> Option<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

/// The "File description" from the executable's version information, such as "Notepad".
fn file_description(path: &str) -> Option<String> {
    let file = wide(path);
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(file.as_ptr()), None) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(PCWSTR(file.as_ptr()), None, size, data.as_mut_ptr().cast()) }
        .ok()?;
    let query = |key: &str| -> Option<(*const c_void, u32)> {
        let key = wide(key);
        let mut value: *mut c_void = std::ptr::null_mut();
        let mut length = 0u32;
        let found = unsafe {
            VerQueryValueW(
                data.as_ptr().cast(),
                PCWSTR(key.as_ptr()),
                &mut value,
                &mut length,
            )
        };
        (found.as_bool() && !value.is_null() && length > 0).then_some((value.cast_const(), length))
    };
    // The first language and code page the file lists.
    let (translation, _) = query("\\VarFileInfo\\Translation")?;
    let pair = unsafe { std::slice::from_raw_parts(translation as *const u16, 2) };
    let key = format!(
        "\\StringFileInfo\\{:04x}{:04x}\\FileDescription",
        pair[0], pair[1]
    );
    let (text, length) = query(&key)?;
    let units = unsafe { std::slice::from_raw_parts(text as *const u16, length as usize) };
    let description = String::from_utf16_lossy(units)
        .trim_end_matches('\0')
        .trim()
        .to_string();
    (!description.is_empty()).then_some(description)
}

fn rect_from(rect: RECT) -> Rect {
    Rect {
        x: f64::from(rect.left),
        y: f64::from(rect.top),
        width: f64::from(rect.right - rect.left),
        height: f64::from(rect.bottom - rect.top),
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn utf16_slice(text: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&units[start.min(units.len())..end.min(units.len())])
}
