mod ax;
mod field;
mod tap;

pub use field::{focused_field, Field, FocusError, InsertError};
pub use tap::EventTap;

use objc2_application_services::{kAXTrustedCheckOptionPrompt, AXIsProcessTrustedWithOptions};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFString};

/// Whether Typio has Accessibility access. With `prompt`, macOS shows its own dialog that
/// points the user to System Settings; it never grants access by itself.
pub fn is_trusted(prompt: bool) -> bool {
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let value = CFBoolean::new(prompt);
    let options = CFDictionary::from_slices(&[key], &[value]);
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
}
