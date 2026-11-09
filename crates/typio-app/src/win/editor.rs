//! The dataset editor: a name and the dataset as text, one shortcut (`trigger: text`) or
//! value per line, the same format Typio imports.

use std::cell::RefCell;

use typio_core::DatasetKind;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetSysColor, GetSysColorBrush, SetBkColor, SetTextColor, COLOR_GRAYTEXT,
    COLOR_WINDOW, COLOR_WINDOWTEXT, HDC, HFONT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, IsWindow, LoadCursorW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow,
    BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL,
    ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN, HWND_TOP, IDC_ARROW, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOZORDER, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CLIPCHILDREN,
    WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_HSCROLL, WS_SYSMENU, WS_TABSTOP, WS_VSCROLL,
};

use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};

use super::controller::with_app;
use super::ui;
use crate::i18n::{tr, trf, T};

/// `SS_LEFT`: left-aligned static text that wraps.
const STATIC_LEFT: WINDOW_STYLE = WINDOW_STYLE(0);
const WIDTH: f64 = 600.0;
const HEIGHT: f64 = 520.0;
const MARGIN: f64 = 16.0;
const KINDS: [(DatasetKind, T); 2] = [
    (DatasetKind::Shortcuts, T::KindShortcuts),
    (DatasetKind::Values, T::KindValues),
];

const ID_NAME: usize = 200;
const ID_KIND: usize = 201;
const ID_TEXT: usize = 202;
/// The dialog ids Windows sends for Enter and Escape.
const ID_SAVE: usize = 1;
const ID_CANCEL: usize = 2;

struct Editor {
    window: HWND,
    name: HWND,
    kind: HWND,
    text: HWND,
    /// The dataset being edited; `None` for a new one.
    dataset: Option<String>,
    fonts: [HFONT; 2],
    help: Vec<HWND>,
}

thread_local! {
    static EDITOR: RefCell<Option<Editor>> = const { RefCell::new(None) };
}

/// The editor window, for keyboard navigation and Ctrl+S in the message loop.
pub fn window() -> Option<HWND> {
    EDITOR.with_borrow(|slot| slot.as_ref().map(|editor| editor.window))
}

/// Opens the editor for the dataset at `index` in the store, or for a new dataset.
pub fn open(index: Option<usize>) {
    let dataset = match index {
        Some(index) => match with_app(|app| app.dataset_for_editing(index)).flatten() {
            Some(found) => Some(found),
            None => return,
        },
        None => None,
    };
    close();

    let instance = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        hIcon: ui::app_icon(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        lpszClassName: w!("TypioEditor"),
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    let title = match &dataset {
        Some((_, name, _, _)) => trf(T::EditTitle, &[("name", name)]),
        None => tr(T::NewDataset).to_string(),
    };
    let title = ui::wide(&title);
    let style = WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let Ok(window) = (unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("TypioEditor"),
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
    let dpi = unsafe { GetDpiForWindow(window) }.max(96);
    let scale = f64::from(dpi) / 96.0;
    let mut frame = RECT {
        left: 0,
        top: 0,
        right: (WIDTH * scale).round() as i32,
        bottom: (HEIGHT * scale).round() as i32,
    };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WS_EX_CONTROLPARENT, dpi);
        let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
        let _ = SetWindowPos(
            window,
            Some(HWND_TOP),
            (GetSystemMetrics(SM_CXSCREEN) - width) / 2,
            (GetSystemMetrics(SM_CYSCREEN) - height) / 2,
            width,
            height,
            SWP_NOZORDER,
        );
    }

    let font = ui::ui_font(scale);
    let mono = ui::font(10.0, scale, w!("Consolas"));
    let add = |class: PCWSTR, text: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE, id, frame| {
        ui::control(window, class, text, style, ex, id, frame, scale, font)
    };
    let inner = WIDTH - 2.0 * MARGIN;
    let none = WINDOW_EX_STYLE(0);
    add(
        w!("STATIC"),
        tr(T::NameLabel),
        STATIC_LEFT,
        none,
        0,
        (MARGIN, 19.0, 70.0, 20.0),
    );
    let name = add(
        w!("EDIT"),
        dataset.as_ref().map_or("", |(_, name, _, _)| name),
        WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_TABSTOP,
        WS_EX_CLIENTEDGE,
        ID_NAME,
        (MARGIN + 80.0, 16.0, inner - 80.0, 24.0),
    );
    add(
        w!("STATIC"),
        tr(T::TypeLabel),
        STATIC_LEFT,
        none,
        0,
        (MARGIN, 55.0, 70.0, 20.0),
    );
    let kind = add(
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_TABSTOP,
        none,
        ID_KIND,
        (MARGIN + 80.0, 52.0, 160.0, 120.0),
    );
    for (_, title) in KINDS {
        let text = ui::wide(tr(title));
        unsafe {
            SendMessageW(
                kind,
                CB_ADDSTRING,
                None,
                Some(LPARAM(text.as_ptr() as isize)),
            );
        }
    }
    let selected = dataset
        .as_ref()
        .and_then(|(_, _, existing, _)| KINDS.iter().position(|(kind, _)| kind == existing))
        .unwrap_or(0);
    unsafe {
        SendMessageW(kind, CB_SETCURSEL, Some(WPARAM(selected)), None);
        if dataset.is_some() {
            // Changing the type would reinterpret every line.
            let _ = EnableWindow(kind, false);
        }
    }
    let help = add(
        w!("STATIC"),
        tr(T::EditorHelp),
        STATIC_LEFT,
        none,
        0,
        (MARGIN, 88.0, inner, 40.0),
    );
    let text = add(
        w!("EDIT"),
        "",
        WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_AUTOHSCROLL | ES_WANTRETURN) as u32)
            | WS_VSCROLL
            | WS_HSCROLL
            | WS_TABSTOP,
        WS_EX_CLIENTEDGE,
        ID_TEXT,
        (MARGIN, 134.0, inner, HEIGHT - 134.0 - 60.0),
    );
    unsafe {
        SendMessageW(
            text,
            WM_SETFONT,
            Some(WPARAM(mono.0 as usize)),
            Some(LPARAM(1)),
        );
        // The edit control wants Windows line breaks.
        let body = dataset
            .as_ref()
            .map_or(String::new(), |(_, _, _, body)| body.replace('\n', "\r\n"));
        let body = ui::wide(&body);
        let _ = SetWindowTextW(text, PCWSTR(body.as_ptr()));
    }
    let buttons_top = HEIGHT - 44.0;
    let hint = add(
        w!("STATIC"),
        &tr(T::SaveHint).replace("⌘S", "Ctrl+S"),
        STATIC_LEFT,
        none,
        0,
        (MARGIN, buttons_top + 6.0, 220.0, 20.0),
    );
    add(
        w!("BUTTON"),
        tr(T::CancelButton),
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        none,
        ID_CANCEL,
        (WIDTH - MARGIN - 208.0, buttons_top, 100.0, 30.0),
    );
    add(
        w!("BUTTON"),
        tr(T::SaveButton),
        WINDOW_STYLE(BS_DEFPUSHBUTTON as u32) | WS_TABSTOP,
        none,
        ID_SAVE,
        (WIDTH - MARGIN - 100.0, buttons_top, 100.0, 30.0),
    );

    EDITOR.with_borrow_mut(|slot| {
        *slot = Some(Editor {
            window,
            name,
            kind,
            text,
            dataset: dataset.map(|(id, _, _, _)| id),
            fonts: [font, mono],
            help: vec![help, hint],
        });
    });
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(if index.is_some() { text } else { name }));
    }
}

/// Saves the dataset, then closes the editor; on a problem, explains it and stays open.
pub fn save() {
    let Some((dataset, name, kind, text)) = EDITOR.with_borrow(|slot| {
        let editor = slot.as_ref()?;
        let index = unsafe { SendMessageW(editor.kind, CB_GETCURSEL, None, None) }.0;
        let kind = usize::try_from(index)
            .ok()
            .and_then(|index| KINDS.get(index))
            .map_or(DatasetKind::Shortcuts, |(kind, _)| *kind);
        Some((
            editor.dataset.clone(),
            ui::window_text(editor.name),
            kind,
            ui::window_text(editor.text).replace("\r\n", "\n"),
        ))
    }) else {
        return;
    };
    let result = with_app(|app| app.save_dataset(dataset.as_deref(), &name, kind, &text));
    match result {
        Some(Ok(warnings)) => {
            close();
            if !warnings.is_empty() {
                let mut shown: Vec<String> = warnings.iter().take(10).cloned().collect();
                if warnings.len() > shown.len() {
                    shown.push(trf(
                        T::AndMore,
                        &[("count", &(warnings.len() - shown.len()).to_string())],
                    ));
                }
                ui::alert(tr(T::SavedWithWarnings), &shown.join("\n"));
            }
        }
        Some(Err(reason)) => ui::alert(tr(T::CannotSaveDataset), &reason),
        None => ui::alert(tr(T::Busy), tr(T::TrySavingAgain)),
    }
}

pub fn close() {
    if let Some(window) = window() {
        if unsafe { IsWindow(Some(window)) }.as_bool() {
            unsafe {
                let _ = DestroyWindow(window);
            }
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
        WM_COMMAND => {
            match wparam.0 & 0xffff {
                ID_SAVE => save(),
                ID_CANCEL => close(),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            let control = HWND(lparam.0 as *mut _);
            let dc = HDC(wparam.0 as *mut _);
            let help = EDITOR.with_borrow(|slot| {
                slot.as_ref()
                    .is_some_and(|editor| editor.help.contains(&control))
            });
            unsafe {
                let color = if help {
                    COLOR_GRAYTEXT
                } else {
                    COLOR_WINDOWTEXT
                };
                SetTextColor(dc, COLORREF(GetSysColor(color)));
                SetBkColor(dc, COLORREF(GetSysColor(COLOR_WINDOW)));
                LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize)
            }
        }
        WM_CLOSE => {
            close();
            LRESULT(0)
        }
        WM_DESTROY => {
            let editor = EDITOR.with_borrow_mut(Option::take);
            if let Some(editor) = editor {
                for font in editor.fonts {
                    unsafe {
                        let _ = DeleteObject(font.into());
                    }
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
