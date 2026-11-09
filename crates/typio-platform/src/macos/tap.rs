//! A global `CGEventTap` that reports key and mouse presses and can swallow them.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRetained, CFRunLoop};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventMask, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType,
};

use super::field::SYNTHETIC_MARK;
use crate::{Disposition, InputEvent, Key, Modifiers};

type Handler = Box<dyn FnMut(InputEvent) -> Disposition>;

struct TapState {
    port: Option<CFRetained<CFMachPort>>,
    handler: Handler,
}

/// Keeps the tap installed on the main run loop for as long as it lives.
pub struct EventTap {
    state: *mut TapState,
}

impl EventTap {
    /// Installs the tap on the main run loop. Needs Accessibility access; fails without it.
    /// `handler` runs on the main thread and must return quickly.
    pub fn install(
        handler: impl FnMut(InputEvent) -> Disposition + 'static,
    ) -> Result<Self, &'static str> {
        let state = Box::into_raw(Box::new(TapState {
            port: None,
            handler: Box::new(handler),
        }));
        let mask: CGEventMask = [
            CGEventType::KeyDown,
            CGEventType::LeftMouseDown,
            CGEventType::RightMouseDown,
            CGEventType::OtherMouseDown,
        ]
        .iter()
        .fold(0, |mask, kind| mask | (1 << kind.0));

        let port = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                mask,
                Some(callback),
                state.cast(),
            )
        };
        let Some(port) = port else {
            drop(unsafe { Box::from_raw(state) });
            return Err("CGEventTapCreate returned no tap (no Accessibility access?)");
        };
        // From here the tap exists; dropping `tap` disables it and frees the state.
        unsafe { (*state).port = Some(port.clone()) };
        let tap = Self { state };
        let source = CFMachPort::new_run_loop_source(None, Some(&port), 0)
            .ok_or("could not create a run loop source for the tap")?;
        CFRunLoop::main()
            .ok_or("no main run loop")?
            .add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
        CGEvent::tap_enable(&port, true);
        Ok(tap)
    }
}

impl Drop for EventTap {
    fn drop(&mut self) {
        let state = unsafe { Box::from_raw(self.state) };
        if let Some(port) = &state.port {
            CGEvent::tap_enable(port, false);
            CFMachPort::invalidate(port);
        }
    }
}

unsafe extern "C-unwind" fn callback(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    let state = unsafe { &mut *user_info.cast::<TapState>() };
    let event_ref = unsafe { event.as_ref() };

    // macOS turns a tap off when it is slow or the user is typing a secure password;
    // turn it back on.
    if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput {
        if let Some(port) = &state.port {
            CGEvent::tap_enable(port, true);
        }
        return event.as_ptr();
    }
    if CGEvent::integer_value_field(Some(event_ref), CGEventField::EventSourceUserData)
        == SYNTHETIC_MARK
    {
        return event.as_ptr();
    }

    let input = if kind == CGEventType::KeyDown {
        let code =
            CGEvent::integer_value_field(Some(event_ref), CGEventField::KeyboardEventKeycode);
        InputEvent::KeyDown {
            key: key_for(code),
            modifiers: modifiers(CGEvent::flags(Some(event_ref))),
        }
    } else {
        let point = CGEvent::location(Some(event_ref));
        InputEvent::MouseDown {
            x: point.x,
            y: point.y,
        }
    };

    match (state.handler)(input) {
        Disposition::Pass => event.as_ptr(),
        Disposition::Swallow => ptr::null_mut(),
    }
}

/// Virtual key codes from `HIToolbox/Events.h`.
fn key_for(code: i64) -> Key {
    match code {
        126 => Key::Up,
        125 => Key::Down,
        36 | 76 => Key::Return,
        48 => Key::Tab,
        53 => Key::Escape,
        17 => Key::T,
        49 => Key::Space,
        51 => Key::Backspace,
        _ => Key::Other,
    }
}

fn modifiers(flags: CGEventFlags) -> Modifiers {
    Modifiers {
        shift: flags.contains(CGEventFlags::MaskShift),
        control: flags.contains(CGEventFlags::MaskControl),
        option: flags.contains(CGEventFlags::MaskAlternate),
        command: flags.contains(CGEventFlags::MaskCommand),
    }
}
