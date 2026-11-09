//! The menu bar icon and its menu, rebuilt each time it opens.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu,
    NSMenuDelegate, NSMenuItem, NSModalResponseOK, NSOpenPanel, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength, NSWorkspace,
};
use objc2_foundation::{NSString, NSURL};
use typio_core::DatasetKind;

use crate::controller::with_app;
use crate::store::Store;

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

        #[unsafe(method(importDataset:))]
        fn import_dataset(&self, _sender: Option<&AnyObject>) {
            let mtm = self.mtm();
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
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                messages.push(match result {
                    Some(Ok(message)) => message,
                    Some(Err(error)) => format!("{name}: {error}"),
                    None => format!("{name}: Typio is busy, try again."),
                });
            }
            alert(mtm, "Import finished", &messages.join("\n"));
        }

        #[unsafe(method(openDataFolder:))]
        fn open_data_folder(&self, _sender: Option<&AnyObject>) {
            let dir = Store::data_dir();
            let _ = std::fs::create_dir_all(&dir);
            let url = NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()));
            NSWorkspace::sharedWorkspace().openURL(&url);
        }

        #[unsafe(method(openAccessibilitySettings:))]
        fn open_accessibility_settings(&self, _sender: Option<&AnyObject>) {
            // Shows Apple's own prompt; access is only ever granted by the user.
            typio_platform::is_trusted(true);
            if let Some(url) = NSURL::URLWithString(&NSString::from_str(ACCESSIBILITY_SETTINGS)) {
                NSWorkspace::sharedWorkspace().openURL(&url);
            }
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

        let state = with_app(|app| {
            let trusted = app.has_tap();
            let datasets: Vec<String> = app
                .store
                .datasets
                .iter()
                .map(|dataset| match dataset.kind() {
                    DatasetKind::Shortcuts => format!(
                        "{} · {} shortcuts",
                        dataset.name,
                        dataset.shortcuts.as_ref().map_or(0, Vec::len)
                    ),
                    DatasetKind::Values => {
                        format!("{} · {} values", dataset.name, dataset.values.len())
                    }
                })
                .collect();
            (trusted, datasets, app.store.enabled)
        });
        let Some((trusted, datasets, enabled)) = state else {
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
            for dataset in &datasets {
                let item = self.item(mtm, dataset, None);
                item.setIndentationLevel(1);
                menu.addItem(&item);
            }
        }
        menu.addItem(&self.item(mtm, "Import Dataset…", Some(sel!(importDataset:))));
        menu.addItem(&self.item(mtm, "Open Data Folder", Some(sel!(openDataFolder:))));
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
pub struct StatusMenu {
    _item: Retained<NSStatusItem>,
    _menu: Retained<NSMenu>,
    _target: Retained<MenuTarget>,
}

pub fn install(mtm: MainThreadMarker) -> StatusMenu {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("character.cursor.ibeam"),
            Some(&NSString::from_str("Typio")),
        );
        match image {
            Some(image) => {
                image.setTemplate(true);
                button.setImage(Some(&image));
            }
            None => button.setTitle(&NSString::from_str("Ty")),
        }
    }
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let target = MenuTarget::new(mtm);
    menu.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*target)));
    item.setMenu(Some(&menu));
    StatusMenu {
        _item: item,
        _menu: menu,
        _target: target,
    }
}

pub fn alert(mtm: MainThreadMarker, title: &str, text: &str) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    NSApplication::sharedApplication(mtm).activate();
    alert.runModal();
}
