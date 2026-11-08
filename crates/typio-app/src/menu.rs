//! The menu bar icon and its menu, rebuilt each time it opens.

use std::cell::RefCell;

use crate::controller::{with_app, UiState};
use crate::store::Store;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu,
    NSMenuDelegate, NSMenuItem, NSModalResponseOK, NSOpenPanel, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength, NSWorkspace,
};
use objc2_foundation::{NSString, NSURL};

thread_local! {
    static STATUS: RefCell<Option<StatusMenu>> = const { RefCell::new(None) };
}

const ACCESSIBILITY_SETTINGS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

define_class!(
    // SAFETY: NSObject has no subclassing requirements and MenuTarget does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TypioMenuTarget"]
    pub struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(toggleEnabled:))]
        fn toggle_enabled(&self, _sender: Option<&AnyObject>) {
            with_app(|app| app.toggle_enabled());
        }

        #[unsafe(method(toggleDataset:))]
        fn toggle_dataset(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let index: isize = unsafe { msg_send![sender, tag] };
            let state: isize = unsafe { msg_send![sender, state] };
            // A menu item still shows its old state when its action runs.
            with_app(|app| app.set_dataset_enabled(index as usize, state != NSControlStateValueOn));
        }

        #[unsafe(method(importDataset:))]
        fn import_dataset(&self, _sender: Option<&AnyObject>) {
            import_datasets(self.mtm());
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            crate::settings::show(self.mtm());
        }

        #[unsafe(method(openAccessibilitySettings:))]
        fn open_accessibility_settings(&self, _sender: Option<&AnyObject>) {
            open_accessibility_settings();
        }
    }

    unsafe impl NSObjectProtocol for MenuTarget {}

    unsafe impl NSMenuDelegate for MenuTarget {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            self.rebuild(menu);
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }

    fn rebuild(&self, menu: &NSMenu) {
        let mtm = self.mtm();
        menu.removeAllItems();

        let Some(UiState {
            trusted,
            enabled,
            datasets,
            ..
        }) = with_app(|app| app.ui_state())
        else {
            return;
        };

        if !trusted {
            menu.addItem(&self.item(mtm, "Typio needs Accessibility access", None));
            menu.addItem(&self.item(
                mtm,
                "Open Accessibility Settings…",
                Some(sel!(openAccessibilitySettings:)),
            ));
        } else {
            let status = if enabled {
                "Typio is on"
            } else {
                "Typio is off"
            };
            menu.addItem(&self.item(mtm, status, None));
            let toggle = self.item(mtm, "Enable Typio", Some(sel!(toggleEnabled:)));
            toggle.setState(if enabled {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            toggle.setKeyEquivalent(&NSString::from_str("t"));
            toggle.setKeyEquivalentModifierMask(
                objc2_app_kit::NSEventModifierFlags::Control
                    | objc2_app_kit::NSEventModifierFlags::Option,
            );
            menu.addItem(&toggle);
        }

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        if datasets.is_empty() {
            menu.addItem(&self.item(mtm, "No datasets yet", None));
        } else {
            menu.addItem(&self.item(mtm, "Datasets", None));
            for (index, dataset) in datasets.iter().enumerate() {
                let item = self.item(
                    mtm,
                    &format!("{} · {}", dataset.name, dataset.detail),
                    Some(sel!(toggleDataset:)),
                );
                item.setTag(index as isize);
                item.setState(if dataset.enabled {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
                menu.addItem(&item);
            }
        }
        menu.addItem(&self.item(mtm, "Import Dataset…", Some(sel!(importDataset:))));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let settings = self.item(mtm, "Settings…", Some(sel!(openSettings:)));
        settings.setKeyEquivalent(&NSString::from_str(","));
        menu.addItem(&settings);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let quit = self.item(mtm, "Quit Typio", Some(sel!(terminate:)));
        unsafe { quit.setTarget(None) };
        quit.setKeyEquivalent(&NSString::from_str("q"));
        menu.addItem(&quit);
    }

    /// A menu item that calls `action` on this target; without an action it is a disabled label.
    fn item(
        &self,
        mtm: MainThreadMarker,
        title: &str,
        action: Option<Sel>,
    ) -> Retained<NSMenuItem> {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                action,
                &NSString::new(),
            )
        };
        if action.is_some() {
            unsafe { item.setTarget(Some(self)) };
        } else {
            item.setEnabled(false);
        }
        item
    }
}

/// The status item and its target; both must stay alive for the menu to work.
struct StatusMenu {
    item: Retained<NSStatusItem>,
    _menu: Retained<NSMenu>,
    _target: Retained<MenuTarget>,
}

pub fn install(mtm: MainThreadMarker) {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let target = MenuTarget::new(mtm);
    menu.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*target)));
    item.setMenu(Some(&menu));
    let status = StatusMenu {
        item,
        _menu: menu,
        _target: target,
    };
    set_icon(mtm, &status, "character.cursor.ibeam", false, "Typio");
    STATUS.with_borrow_mut(|slot| *slot = Some(status));
}

/// On: the Typio symbol. Off: the same, dimmed. No Accessibility access: a warning sign.
pub fn update_icon(mtm: MainThreadMarker, state: &UiState) {
    let (symbol, dimmed, tip) = match (state.trusted, state.enabled) {
        (false, _) => (
            "exclamationmark.triangle",
            false,
            "Typio needs Accessibility access",
        ),
        (true, true) => ("character.cursor.ibeam", false, "Typio is on"),
        (true, false) => ("character.cursor.ibeam", true, "Typio is off"),
    };
    STATUS.with_borrow(|status| {
        if let Some(status) = status {
            set_icon(mtm, status, symbol, dimmed, tip);
        }
    });
}

fn set_icon(mtm: MainThreadMarker, status: &StatusMenu, symbol: &str, dimmed: bool, tip: &str) {
    let Some(button) = status.item.button(mtm) else {
        return;
    };
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(symbol),
        Some(&NSString::from_str(tip)),
    );
    match image {
        Some(image) => {
            image.setTemplate(true);
            button.setImage(Some(&image));
        }
        None => button.setTitle(&NSString::from_str("Ty")),
    }
    button.setAppearsDisabled(dimmed);
    button.setToolTip(Some(&NSString::from_str(tip)));
}

/// Asks for files and imports each, then reports what happened.
pub fn import_datasets(mtm: MainThreadMarker) {
    NSApplication::sharedApplication(mtm).activate();
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setAllowsMultipleSelection(true);
    panel.setMessage(Some(&NSString::from_str(
        "Choose TXT, CSV or JSON files, or a Typio extension backup.",
    )));
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    let mut messages = Vec::new();
    for url in panel.URLs().iter() {
        let Some(path) = url.path() else { continue };
        let path = std::path::PathBuf::from(path.to_string());
        let result = with_app(|app| {
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
            None => format!("{name}: Typio is busy, try again."),
        });
    }
    alert(mtm, "Import finished", &messages.join("\n"));
}

pub fn open_data_folder() {
    let dir = Store::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let url = NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()));
    NSWorkspace::sharedWorkspace().openURL(&url);
}

pub fn open_accessibility_settings() {
    // Shows Apple's own prompt; access is only ever granted by the user.
    typio_platform::is_trusted(true);
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(ACCESSIBILITY_SETTINGS)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

pub fn alert(mtm: MainThreadMarker, title: &str, text: &str) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    NSApplication::sharedApplication(mtm).activate();
    alert.runModal();
}
