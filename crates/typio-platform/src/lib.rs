//! Operating-system access for Typio: the text field the user is typing in, global key
//! and mouse events, and writing text back. Only macOS is implemented so far.
//!
//! Screen coordinates are in points with the origin at the top-left of the primary
//! display, which is what macOS Accessibility and Windows UI Automation both report.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

/// A rectangle in screen points, origin top-left of the primary display.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

/// What Typio knows about the focused text field.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FieldInfo {
    /// Bundle identifier (macOS) or executable name of the app that owns the field.
    pub app_id: String,
    pub app_name: String,
    pub pid: i32,
    /// Native role, for example `AXTextField` or `AXTextArea`.
    pub role: String,
    /// A stable identifier the app gives the field, when it has one.
    pub identifier: String,
    /// Description, title and placeholder joined, for matching and sensitive checks.
    pub label: String,
    pub value: String,
    /// Caret offset in UTF-16 units; `None` when text is selected or the app hides it.
    pub caret: Option<usize>,
    pub caret_rect: Option<Rect>,
    pub field_rect: Option<Rect>,
    /// A password field. Typio never reads from or suggests in these.
    pub secure: bool,
}

/// Modifier keys held during a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub option: bool,
    pub command: bool,
}

/// Keys the suggestion popup reacts to; everything else is `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Return,
    Tab,
    Escape,
    /// The physical T key, used by the enable shortcut.
    T,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    KeyDown {
        key: Key,
        modifiers: Modifiers,
    },
    /// A mouse button went down at screen point (`x`, `y`).
    MouseDown {
        x: f64,
        y: f64,
    },
}

/// Whether an input event continues to the focused app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Pass,
    Swallow,
}
