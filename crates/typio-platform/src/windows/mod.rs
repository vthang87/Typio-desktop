mod field;
mod hook;

pub use field::{focused_field, frontmost_app, press_backspace, Field, FocusError, InsertError};
pub use hook::EventTap;

/// Windows needs no permission to read other apps' fields or watch the keyboard.
pub fn is_trusted(_prompt: bool) -> bool {
    true
}

/// Marks input Typio sends so the keyboard hook can tell it from the user's own typing.
pub(crate) const SYNTHETIC_MARK: usize = 0x5459_5049;
