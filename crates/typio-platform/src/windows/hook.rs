//! Low-level keyboard and mouse hooks on a thread of their own, so slow work on the main
//! thread (UI Automation calls) never delays the user's input.

use std::cell::RefCell;
use std::sync::mpsc;
use std::thread::JoinHandle;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LWIN, VK_MENU,
    VK_RETURN, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
    WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN,
};

use super::SYNTHETIC_MARK;
use crate::{Disposition, InputEvent, Key, Modifiers};

type Handler = Box<dyn FnMut(InputEvent) -> Disposition + Send>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = const { RefCell::new(None) };
}

/// Keeps the hooks installed for as long as it lives.
pub struct EventTap {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl EventTap {
    /// Installs the hooks on a new thread. `handler` runs on that thread for every key and
    /// mouse press and must return quickly: Windows passes the input on by itself when a
    /// hook takes too long, and removes hooks that keep doing so.
    pub fn install(
        handler: impl FnMut(InputEvent) -> Disposition + Send + 'static,
    ) -> Result<Self, &'static str> {
        let (ready, started) = mpsc::channel();
        let handler: Handler = Box::new(handler);
        let thread = std::thread::Builder::new()
            .name("typio-hook".into())
            .spawn(move || run(handler, ready))
            .map_err(|_| "could not start the hook thread")?;
        match started.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err("the hook thread stopped"),
        }
    }
}

impl Drop for EventTap {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(handler: Handler, ready: mpsc::Sender<Result<u32, &'static str>>) {
    HANDLER.with_borrow_mut(|slot| *slot = Some(handler));
    let module = unsafe { GetModuleHandleW(None) }.ok().map(Into::into);
    let keyboard = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) };
    let mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) };
    let (keyboard, mouse) = match (keyboard, mouse) {
        (Ok(keyboard), Ok(mouse)) => (keyboard, mouse),
        (keyboard, mouse) => {
            for hook in [keyboard, mouse].into_iter().flatten() {
                let _ = unsafe { UnhookWindowsHookEx(hook) };
            }
            let _ = ready.send(Err("SetWindowsHookEx failed"));
            return;
        }
    };
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));

    // Hooks are called from this thread's message loop.
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe {
        let _ = UnhookWindowsHookEx(keyboard);
        let _ = UnhookWindowsHookEx(mouse);
    }
}

fn call_handler(event: InputEvent) -> Disposition {
    HANDLER.with(|slot| match slot.try_borrow_mut() {
        Ok(mut handler) => handler
            .as_mut()
            .map_or(Disposition::Pass, |handler| handler(event)),
        Err(_) => Disposition::Pass,
    })
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let message = wparam.0 as u32;
        if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            if info.dwExtraInfo != SYNTHETIC_MARK {
                let mut key = key_for(VIRTUAL_KEY(info.vkCode as u16));
                // Input methods (such as Vietnamese Telex) send their own Backspaces; only
                // a Backspace the user pressed may undo an expansion.
                if key == Key::Backspace && info.flags.contains(LLKHF_INJECTED) {
                    key = Key::Other;
                }
                let event = InputEvent::KeyDown {
                    key,
                    modifiers: modifiers(),
                };
                if call_handler(event) == Disposition::Swallow {
                    return LRESULT(1);
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let message = wparam.0 as u32;
        if message == WM_LBUTTONDOWN || message == WM_RBUTTONDOWN || message == WM_MBUTTONDOWN {
            let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            let event = InputEvent::MouseDown {
                x: f64::from(info.pt.x),
                y: f64::from(info.pt.y),
            };
            if call_handler(event) == Disposition::Swallow {
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn key_for(key: VIRTUAL_KEY) -> Key {
    match key {
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_RETURN => Key::Return,
        VK_TAB => Key::Tab,
        VK_ESCAPE => Key::Escape,
        VIRTUAL_KEY(0x54) => Key::T,
        VK_SPACE => Key::Space,
        VK_BACK => Key::Backspace,
        _ => Key::Other,
    }
}

/// Control maps to `control`, Alt to `option` and the Windows key to `command`.
fn modifiers() -> Modifiers {
    let down = |key: VIRTUAL_KEY| unsafe { GetAsyncKeyState(i32::from(key.0)) } < 0;
    Modifiers {
        shift: down(VK_SHIFT),
        control: down(VK_CONTROL),
        option: down(VK_MENU),
        command: down(VK_LWIN) || down(VK_RWIN),
    }
}
