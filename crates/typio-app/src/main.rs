//! Typio for macOS: a menu bar app that suggests values and expands shortcuts in the text
//! fields the user turns it on for.

/// Always logged: start-up, permissions and failures. Never pass typed text or field values.
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::write(format_args!($($arg)*))
    };
}

/// Logged only in verbose mode (see `log::verbose`). Never pass typed text or field values.
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::log::verbose() {
            $crate::log::write(format_args!($($arg)*));
        }
    };
}

#[cfg(target_os = "macos")]
mod controller;
mod log;
#[cfg(target_os = "macos")]
mod menu;
#[cfg(target_os = "macos")]
mod popup;
mod store;

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let mtm = MainThreadMarker::new().expect("Typio starts on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // A menu bar app: no Dock icon and no main menu.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let store = match store::Store::load(&store::Store::data_dir()) {
        Ok(store) => store,
        Err(error) => {
            // Never overwrite data Typio cannot read.
            menu::alert(
                mtm,
                "Typio could not read its data",
                &format!(
                    "{error}\n\nThe file is in {}.",
                    store::Store::data_dir().display()
                ),
            );
            return;
        }
    };
    controller::install(store, mtm);
    let _menu = menu::install(mtm);
    let executable = std::env::current_exe().unwrap_or_default();
    info!(
        "Typio {} started from {}; Accessibility access: {}",
        env!("CARGO_PKG_VERSION"),
        executable.display(),
        typio_platform::is_trusted(true)
    );
    controller::with_app(|app| app.start_tap());
    app.run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Typio desktop currently runs on macOS only.");
}
