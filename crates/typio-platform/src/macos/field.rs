//! The focused text field: reading it and writing text into it.

use std::collections::HashSet;
use std::sync::Mutex;

use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_core_foundation::{CFIndex, CFRange};
use objc2_core_graphics::{CGEvent, CGEventField};

use super::ax::Element;
use crate::{FieldInfo, Rect};

const TEXT_ROLES: [&str; 4] = ["AXTextField", "AXTextArea", "AXComboBox", "AXSearchField"];
const SECURE_ROLE: &str = "AXSecureTextField";
const BACKSPACE_KEY: u16 = 51;
/// Marks events Typio posts so they can be told apart from the user's own typing.
pub(crate) const SYNTHETIC_MARK: i64 = 0x5459_5049;

/// Apps that only build their accessibility tree when asked (Electron); asked once each.
static ENABLED_PIDS: Mutex<Option<HashSet<i32>>> = Mutex::new(None);

/// A focused text field: its details and a handle for writing into it.
#[derive(Debug, Clone)]
pub struct Field {
    pub info: FieldInfo,
    element: Element,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertError {
    /// The field cannot be edited through Accessibility and keystrokes could not be sent.
    Unsupported,
}

/// Why there is no field to work with; logged to diagnose apps that hide their fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusError {
    /// Neither the system nor the frontmost app reports a focused element. `code` is the
    /// `AXError` the frontmost app returned.
    NoFocusedElement { app_id: Option<String>, code: i32 },
    /// Focus is on something that is not a text field.
    NotEditable {
        app_id: String,
        role: String,
        subrole: String,
    },
    /// The focused element does not say which process owns it.
    NoProcess,
}

/// The text field with keyboard focus.
pub fn focused_field() -> Result<Field, FocusError> {
    let system = Element::system_wide();
    // The frontmost app from AppKit works even when the app ignores system-wide queries.
    let pid = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier());
    let app = pid.map(Element::application);
    // Some apps answer only when asked through their own application element.
    let focused = system
        .element("AXFocusedUIElement")
        .or_else(|| app.as_ref()?.element("AXFocusedUIElement"));
    let Some(element) = focused else {
        let code = app
            .as_ref()
            .map_or(0, |app| app.error_code("AXFocusedUIElement"));
        if let Some(pid) = pid {
            enable_app_accessibility(pid);
        }
        return Err(FocusError::NoFocusedElement {
            app_id: pid.map(|pid| app_identity(pid).0),
            code,
        });
    };

    let role = element.string("AXRole").unwrap_or_default();
    let subrole = element.string("AXSubrole").unwrap_or_default();
    let secure = role == SECURE_ROLE || subrole == SECURE_ROLE;
    let editable =
        TEXT_ROLES.contains(&role.as_str()) || element.bool("AXEditable").unwrap_or(false);
    if !editable && !secure {
        let app_id = element
            .pid()
            .map(|pid| app_identity(pid).0)
            .unwrap_or_default();
        return Err(FocusError::NotEditable {
            app_id,
            role,
            subrole,
        });
    }

    let pid = element.pid().ok_or(FocusError::NoProcess)?;
    let (app_id, app_name) = app_identity(pid);
    let label = ["AXDescription", "AXTitle", "AXPlaceholderValue"]
        .iter()
        .filter_map(|name| element.string(name))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    let mut info = FieldInfo {
        app_id,
        app_name,
        pid,
        role,
        identifier: element.string("AXIdentifier").unwrap_or_default(),
        label,
        secure,
        field_rect: element.frame(),
        ..FieldInfo::default()
    };
    // Password fields are never read.
    if !secure {
        info.value = element.string("AXValue").unwrap_or_default();
        info.caret = element
            .range("AXSelectedTextRange")
            .filter(|range| range.length == 0)
            .and_then(|range| usize::try_from(range.location).ok());
        info.caret_rect = info.caret.and_then(|caret| caret_rect(&element, caret));
    }

    Ok(Field { info, element })
}

impl Field {
    /// Replaces `length` UTF-16 units at `start` with `text` and leaves the caret after it.
    pub fn replace(&self, start: usize, length: usize, text: &str) -> Result<(), InsertError> {
        let selected = self
            .element
            .set_range("AXSelectedTextRange", cf_range(start, length));

        // Native Cocoa fields accept the text directly; this keeps undo working.
        if selected && self.element.set_string("AXSelectedText", text) {
            let expected = splice(&self.info.value, start, length, text);
            if self.element.string("AXValue").as_deref() == Some(expected.as_str()) {
                let end = start + text.encode_utf16().count();
                self.element
                    .set_range("AXSelectedTextRange", cf_range(end, 0));
                return Ok(());
            }
            // Some apps report success without changing anything; only fall through if
            // the field still holds its old text, so nothing is inserted twice.
            if self.element.string("AXValue").as_deref() != Some(self.info.value.as_str()) {
                return Ok(());
            }
        }

        // Otherwise type it: with the range selected, typing replaces it; without, delete
        // back from the caret first.
        let selection_matches = selected
            && self
                .element
                .range("AXSelectedTextRange")
                .is_some_and(|range| {
                    range.location == start as CFIndex && range.length == length as CFIndex
                });
        if !selection_matches {
            if self.info.caret != Some(start + length) {
                return Err(InsertError::Unsupported);
            }
            let removed = utf16_slice(&self.info.value, start, start + length);
            for _ in removed.chars() {
                post_key(self.info.pid, BACKSPACE_KEY, None)?;
            }
        }
        let units: Vec<u16> = text.encode_utf16().collect();
        // Apps read at most 20 UTF-16 units from one event.
        for chunk in units.chunks(20) {
            post_key(self.info.pid, 0, Some(chunk))?;
        }
        Ok(())
    }
}

fn caret_rect(element: &Element, caret: usize) -> Option<Rect> {
    let usable = |rect: &Rect| rect.height > 0.0 && (rect.x != 0.0 || rect.y != 0.0);
    if let Some(rect) = element.rect_for_range(cf_range(caret, 0)).filter(usable) {
        return Some(rect);
    }
    // Many apps return nothing for an empty range; use the character before the caret.
    let previous = caret.checked_sub(1)?;
    element
        .rect_for_range(cf_range(previous, 1))
        .filter(usable)
        .map(|rect| Rect {
            x: rect.x + rect.width,
            width: 1.0,
            ..rect
        })
}

fn app_identity(pid: i32) -> (String, String) {
    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
        return (format!("pid:{pid}"), String::new());
    };
    let id = app
        .bundleIdentifier()
        .map(|id| id.to_string())
        .unwrap_or_else(|| format!("pid:{pid}"));
    let name = app
        .localizedName()
        .map(|name| name.to_string())
        .unwrap_or_default();
    (id, name)
}

/// Electron apps expose text fields only after `AXManualAccessibility` is set, Chromium
/// browsers after `AXEnhancedUserInterface`. Asked once per process; takes effect on the
/// next read.
fn enable_app_accessibility(pid: i32) {
    let mut enabled = ENABLED_PIDS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if enabled.get_or_insert_with(HashSet::new).insert(pid) {
        let app = Element::application(pid);
        app.set_bool("AXManualAccessibility", true);
        app.set_bool("AXEnhancedUserInterface", true);
    }
}

fn post_key(pid: i32, key: u16, text: Option<&[u16]>) -> Result<(), InsertError> {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(None, key, down).ok_or(InsertError::Unsupported)?;
        if let Some(text) = text {
            unsafe {
                CGEvent::keyboard_set_unicode_string(Some(&event), text.len() as _, text.as_ptr())
            };
        }
        CGEvent::set_integer_value_field(
            Some(&event),
            CGEventField::EventSourceUserData,
            SYNTHETIC_MARK,
        );
        // Straight to the app, past input methods that would treat it as a key press.
        CGEvent::post_to_pid(pid, Some(&event));
    }
    Ok(())
}

fn cf_range(location: usize, length: usize) -> CFRange {
    CFRange {
        location: location as CFIndex,
        length: length as CFIndex,
    }
}

fn utf16_slice(text: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&units[start.min(units.len())..end.min(units.len())])
}

fn splice(text: &str, start: usize, length: usize, insert: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let total = units.len();
    let before = String::from_utf16_lossy(&units[..start.min(total)]);
    let after = String::from_utf16_lossy(&units[(start + length).min(total)..]);
    format!("{before}{insert}{after}")
}
