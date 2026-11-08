//! The Settings window: General (on/off, trigger, Accessibility), Datasets and Apps tabs. Its content is rebuilt from `UiState` whenever anything changes.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSBackingStoreType, NSBorderType, NSButton,
    NSColor, NSControlSize, NSControlStateValueOff, NSControlStateValueOn, NSFont, NSLineBreakMode,
    NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSSavePanel, NSScrollView, NSTabView,
    NSTabViewItem, NSTextAlignment, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSBundle, NSPoint, NSRect, NSSize, NSString, NSURL};

use crate::controller::{with_app, UiState};
use crate::menu;
use crate::store::{AppMode, TRIGGERS};

const WIDTH: f64 = 580.0;
const HEIGHT: f64 = 460.0;
const PAGE_MARGIN: f64 = 16.0;
const ROW_HEIGHT: f64 = 32.0;

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

struct Settings {
    window: Retained<NSWindow>,
    target: Retained<SettingsTarget>,
    tabs: RefCell<Option<Retained<NSTabView>>>,
}

define_class!(
    /// A view whose y axis runs down, so layout reads top to bottom.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "TypioFlippedView"]
    pub(crate) struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }

    unsafe impl NSObjectProtocol for FlippedView {}
);

impl FlippedView {
    pub(crate) fn new(mtm: MainThreadMarker, size: NSSize) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), size)] }
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and SettingsTarget does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TypioSettingsTarget"]
    struct SettingsTarget;

    impl SettingsTarget {
        #[unsafe(method(toggleEnabled:))]
        fn toggle_enabled(&self, _sender: Option<&AnyObject>) {
            with_app(|app| app.toggle_enabled());
        }

        #[unsafe(method(toggleDataset:))]
        fn toggle_dataset(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let index: isize = unsafe { msg_send![sender, tag] };
            let state: isize = unsafe { msg_send![sender, state] };
            with_app(|app| app.set_dataset_enabled(index as usize, state == NSControlStateValueOn));
        }

        #[unsafe(method(deleteDataset:))]
        fn delete_dataset(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let index: isize = unsafe { msg_send![sender, tag] };
            let index = index as usize;
            let Some(Some(name)) =
                with_app(|app| app.store.datasets.get(index).map(|dataset| dataset.name.clone()))
            else {
                return;
            };
            if confirm_delete(self.mtm(), &name) {
                with_app(|app| app.remove_dataset(index));
            }
        }

        #[unsafe(method(changeTrigger:))]
        fn change_trigger(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let index: isize = unsafe { msg_send![sender, indexOfSelectedItem] };
            if let Some(&trigger) = usize::try_from(index).ok().and_then(|index| TRIGGERS.get(index)) {
                with_app(|app| app.set_trigger(trigger));
            }
        }

        #[unsafe(method(editDataset:))]
        fn edit_dataset(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let index: isize = unsafe { msg_send![sender, tag] };
            crate::editor::open(self.mtm(), Some(index as usize));
        }

        #[unsafe(method(newDataset:))]
        fn new_dataset(&self, _sender: Option<&AnyObject>) {
            crate::editor::open(self.mtm(), None);
        }

        #[unsafe(method(exportBackup:))]
        fn export_backup(&self, _sender: Option<&AnyObject>) {
            export_backup(self.mtm());
        }

        #[unsafe(method(removeApp:))]
        fn remove_app(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let index: isize = unsafe { msg_send![sender, tag] };
            with_app(|app| app.remove_app(index as usize));
        }

        #[unsafe(method(addApp:))]
        fn add_app(&self, _sender: Option<&AnyObject>) {
            add_apps(self.mtm());
        }

        #[unsafe(method(changeAppMode:))]
        fn change_app_mode(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let index: isize = unsafe { msg_send![sender, indexOfSelectedItem] };
            let mode = if index == 1 { AppMode::Selected } else { AppMode::All };
            with_app(|app| app.set_app_mode(mode));
        }

        #[unsafe(method(importDataset:))]
        fn import_dataset(&self, _sender: Option<&AnyObject>) {
            menu::import_datasets(self.mtm());
        }

        #[unsafe(method(openDataFolder:))]
        fn open_data_folder(&self, _sender: Option<&AnyObject>) {
            menu::open_data_folder();
        }

        #[unsafe(method(openAccessibilitySettings:))]
        fn open_accessibility_settings(&self, _sender: Option<&AnyObject>) {
            menu::open_accessibility_settings();
        }
    }

    unsafe impl NSObjectProtocol for SettingsTarget {}
);

impl SettingsTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Opens the Settings window, creating it the first time.
pub fn show(mtm: MainThreadMarker) {
    let Some(state) = with_app(|app| app.ui_state()) else {
        return;
    };
    SETTINGS.with_borrow_mut(|slot| {
        let settings = slot.get_or_insert_with(|| {
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT)),
                    NSWindowStyleMask::Titled
                        | NSWindowStyleMask::Closable
                        | NSWindowStyleMask::Miniaturizable,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            unsafe { window.setReleasedWhenClosed(false) };
            window.setTitle(&NSString::from_str("Typio Settings"));
            window.center();
            Settings {
                window,
                target: SettingsTarget::new(mtm),
                tabs: RefCell::new(None),
            }
        });
        render(mtm, settings, &state);
        NSApplication::sharedApplication(mtm).activate();
        settings.window.makeKeyAndOrderFront(None);
    });
}

/// Redraws the window if it is open.
pub fn update(mtm: MainThreadMarker, state: &UiState) {
    SETTINGS.with_borrow(|slot| {
        if let Some(settings) = slot.as_ref().filter(|settings| settings.window.isVisible()) {
            render(mtm, settings, state);
        }
    });
}

fn render(mtm: MainThreadMarker, settings: &Settings, state: &UiState) {
    let target = &*settings.target;
    let content = FlippedView::new(mtm, NSSize::new(WIDTH, HEIGHT));
    let tabs = NSTabView::initWithFrame(
        NSTabView::alloc(mtm),
        rect(12.0, 10.0, WIDTH - 24.0, HEIGHT - 22.0),
    );
    let page = tabs.contentRect().size;
    for (title, view) in [
        ("General", general_page(mtm, state, target, page)),
        ("Datasets", datasets_page(mtm, state, target, page)),
        ("Apps", apps_page(mtm, state, target, page)),
    ] {
        let item = unsafe { NSTabViewItem::initWithIdentifier(NSTabViewItem::alloc(), None) };
        item.setLabel(&NSString::from_str(title));
        item.setView(Some(&view));
        tabs.addTabViewItem(&item);
    }
    // Keep the tab the user is on across redraws.
    let selected = settings.tabs.borrow().as_ref().map_or(0, |old| {
        old.selectedTabViewItem()
            .map_or(0, |item| old.indexOfTabViewItem(&item))
    });
    tabs.selectTabViewItemAtIndex(selected.max(0));
    add(&content, &tabs);
    settings.window.setContentView(Some(&content));
    *settings.tabs.borrow_mut() = Some(tabs);
}

fn general_page(
    mtm: MainThreadMarker,
    state: &UiState,
    target: &SettingsTarget,
    page: NSSize,
) -> Retained<FlippedView> {
    let view = FlippedView::new(mtm, page);
    let inner = page.width - PAGE_MARGIN * 2.0;
    add(
        &view,
        &checkbox(
            mtm,
            "Enable Typio in all text fields   ⌃⌥T",
            state.enabled,
            target,
            sel!(toggleEnabled:),
            rect(PAGE_MARGIN, 16.0, inner, 20.0),
        ),
    );

    add(
        &view,
        &label(
            mtm,
            "Trigger character",
            rect(PAGE_MARGIN, 53.0, 130.0, 18.0),
        ),
    );
    let trigger = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(PAGE_MARGIN + 130.0, 48.0, 64.0, 26.0),
        false,
    );
    for character in TRIGGERS {
        trigger.addItemWithTitle(&NSString::from_str(&character.to_string()));
    }
    if let Some(index) = TRIGGERS
        .iter()
        .position(|&character| character == state.trigger)
    {
        trigger.selectItemAtIndex(index as isize);
    }
    unsafe {
        trigger.setTarget(Some(target));
        trigger.setAction(Some(sel!(changeTrigger:)));
    }
    add(&view, &trigger);
    add(
        &view,
        &secondary(
            mtm,
            &format!("Type {}xtt to see suggestions for xtt", state.trigger),
            rect(PAGE_MARGIN + 204.0, 54.0, inner - 204.0, 16.0),
        ),
    );

    add(
        &view,
        &secondary(
            mtm,
            "Quick Search: press ⌃⌥Space in any text field to search every enabled dataset.",
            rect(PAGE_MARGIN, 132.0, inner, 16.0),
        ),
    );

    let access = if state.trusted {
        "Accessibility access: granted"
    } else {
        "Accessibility access: not granted"
    };
    add(
        &view,
        &label(mtm, access, rect(PAGE_MARGIN, 92.0, 240.0, 18.0)),
    );
    if !state.trusted {
        add(
            &view,
            &button(
                mtm,
                "Open Accessibility Settings…",
                target,
                sel!(openAccessibilitySettings:),
                rect(page.width - PAGE_MARGIN - 230.0, 86.0, 230.0, 28.0),
            ),
        );
    }
    view
}

fn datasets_page(
    mtm: MainThreadMarker,
    state: &UiState,
    target: &SettingsTarget,
    page: NSSize,
) -> Retained<FlippedView> {
    let view = FlippedView::new(mtm, page);
    let inner = page.width - PAGE_MARGIN * 2.0;
    add(
        &view,
        &secondary(
            mtm,
            "Only checked datasets give suggestions.",
            rect(PAGE_MARGIN, 12.0, inner, 16.0),
        ),
    );
    let list_height = page.height - 36.0 - 56.0;
    let list = list(
        mtm,
        rect(PAGE_MARGIN, 36.0, inner, list_height),
        state.datasets.len(),
        "No datasets yet. Click Import… to add a TXT, CSV or JSON file.",
        |list, index, top, width| {
            let dataset = &state.datasets[index];
            let enable = checkbox(
                mtm,
                &dataset.name,
                dataset.enabled,
                target,
                sel!(toggleDataset:),
                rect(12.0, top + 6.0, width - 320.0, 20.0),
            );
            enable.setTag(index as isize);
            enable.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            add(list, &enable);

            let detail = secondary(
                mtm,
                &dataset.detail,
                rect(width - 300.0, top + 8.0, 130.0, 16.0),
            );
            detail.setAlignment(NSTextAlignment::Right);
            add(list, &detail);

            add(
                list,
                &small_button(
                    mtm,
                    "Edit…",
                    target,
                    sel!(editDataset:),
                    index,
                    rect(width - 160.0, top + 4.0, 68.0, 24.0),
                ),
            );
            add(
                list,
                &small_button(
                    mtm,
                    "Delete",
                    target,
                    sel!(deleteDataset:),
                    index,
                    rect(width - 88.0, top + 4.0, 76.0, 24.0),
                ),
            );
        },
    );
    add(&view, &list);

    let buttons_top = 36.0 + list_height + 12.0;
    let mut x = PAGE_MARGIN - 6.0;
    for (title, action, width) in [
        ("New…", sel!(newDataset:), 80.0),
        ("Import…", sel!(importDataset:), 90.0),
        ("Export Backup…", sel!(exportBackup:), 130.0),
        ("Open Data Folder", sel!(openDataFolder:), 140.0),
    ] {
        add(
            &view,
            &button(
                mtm,
                title,
                target,
                action,
                rect(x, buttons_top, width, 32.0),
            ),
        );
        x += width + 4.0;
    }
    view
}

fn apps_page(
    mtm: MainThreadMarker,
    state: &UiState,
    target: &SettingsTarget,
    page: NSSize,
) -> Retained<FlippedView> {
    let view = FlippedView::new(mtm, page);
    let inner = page.width - PAGE_MARGIN * 2.0;

    add(
        &view,
        &label(mtm, "Typio runs in", rect(PAGE_MARGIN, 17.0, 100.0, 18.0)),
    );
    let mode = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(PAGE_MARGIN + 100.0, 12.0, 240.0, 26.0),
        false,
    );
    for title in ["All apps except skipped ones", "Only the selected apps"] {
        mode.addItemWithTitle(&NSString::from_str(title));
    }
    mode.selectItemAtIndex(match state.app_mode {
        AppMode::All => 0,
        AppMode::Selected => 1,
    });
    unsafe {
        mode.setTarget(Some(target));
        mode.setAction(Some(sel!(changeAppMode:)));
    }
    add(&view, &mode);

    let (about, empty) = match state.app_mode {
        AppMode::All => (
            "Skipped apps. Browsers start here so the Typio extension handles them. You can also skip the app in use from the Typio menu.",
            "Typio runs in every app.",
        ),
        AppMode::Selected => (
            "Selected apps. Typio runs only in these. You can also add the app in use from the Typio menu.",
            "No apps selected yet, so Typio runs nowhere. Click Add App…",
        ),
    };
    let intro = secondary(mtm, about, rect(PAGE_MARGIN, 48.0, inner, 32.0));
    intro.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    intro.setMaximumNumberOfLines(2);
    add(&view, &intro);

    let list_height = page.height - 88.0 - 56.0;
    let list = list(
        mtm,
        rect(PAGE_MARGIN, 88.0, inner, list_height),
        state.apps.len(),
        empty,
        |list, index, top, width| {
            let app = &state.apps[index];
            add(
                list,
                &label(mtm, &app.name, rect(12.0, top + 7.0, 200.0, 18.0)),
            );
            add(
                list,
                &secondary(
                    mtm,
                    &app.id,
                    rect(216.0, top + 8.0, width - 216.0 - 100.0, 16.0),
                ),
            );
            add(
                list,
                &small_button(
                    mtm,
                    "Remove",
                    target,
                    sel!(removeApp:),
                    index,
                    rect(width - 88.0, top + 4.0, 76.0, 24.0),
                ),
            );
        },
    );
    add(&view, &list);
    add(
        &view,
        &button(
            mtm,
            "Add App…",
            target,
            sel!(addApp:),
            rect(PAGE_MARGIN - 6.0, 88.0 + list_height + 12.0, 110.0, 32.0),
        ),
    );
    view
}

/// Asks for apps and adds each to the current mode's list.
fn add_apps(mtm: MainThreadMarker) {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setAllowsMultipleSelection(true);
    panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(
        "/Applications",
    ))));
    panel.setMessage(Some(&NSString::from_str("Choose apps.")));
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    for url in panel.URLs().iter() {
        let Some(path) = url.path() else { continue };
        let Some(id) = NSBundle::bundleWithPath(&path).and_then(|bundle| bundle.bundleIdentifier())
        else {
            continue;
        };
        let name = std::path::Path::new(&path.to_string())
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| id.to_string());
        with_app(|app| app.add_app(&id.to_string(), &name));
    }
}

/// A scrolling list of `count` rows drawn by `row`, or `empty` when there are none.
fn list(
    mtm: MainThreadMarker,
    frame: NSRect,
    count: usize,
    empty: &str,
    row: impl Fn(&FlippedView, usize, f64, f64),
) -> Retained<NSScrollView> {
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame);
    scroll.setBorderType(NSBorderType::BezelBorder);
    scroll.setHasVerticalScroller(true);
    let width = scroll.contentSize().width;
    let height = (count as f64 * ROW_HEIGHT).max(scroll.contentSize().height);
    let view = FlippedView::new(mtm, NSSize::new(width, height));
    if count == 0 {
        add(
            &view,
            &secondary(mtm, empty, rect(12.0, 12.0, width - 24.0, 16.0)),
        );
    }
    for index in 0..count {
        row(&view, index, index as f64 * ROW_HEIGHT, width);
    }
    scroll.setDocumentView(Some(&view));
    scroll
}

fn small_button(
    mtm: MainThreadMarker,
    title: &str,
    target: &SettingsTarget,
    action: Sel,
    tag: usize,
    frame: NSRect,
) -> Retained<NSButton> {
    let button = button(mtm, title, target, action, frame);
    button.setControlSize(NSControlSize::Small);
    button.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    button.setTag(tag as isize);
    button
}

/// Asks where to save, then writes every dataset as a backup file.
fn export_backup(mtm: MainThreadMarker) {
    let Some(backup) = with_app(|app| app.store.backup()) else {
        return;
    };
    let panel = NSSavePanel::savePanel(mtm);
    let date = humantime::format_rfc3339(std::time::SystemTime::now()).to_string();
    panel.setNameFieldStringValue(&NSString::from_str(&format!(
        "typio-datasets-{}.json",
        &date[..10]
    )));
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    let Some(path) = panel.URL().and_then(|url| url.path()) else {
        return;
    };
    if let Err(error) = std::fs::write(path.to_string(), backup) {
        menu::alert(mtm, "Typio could not save the backup", &error.to_string());
    }
}

fn confirm_delete(mtm: MainThreadMarker, name: &str) -> bool {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&format!("Delete “{name}”?")));
    alert.setInformativeText(&NSString::from_str(
        "Its values and shortcuts are removed from Typio. This cannot be undone.",
    ));
    let delete = alert.addButtonWithTitle(&NSString::from_str("Delete"));
    delete.setHasDestructiveAction(true);
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.runModal() == NSAlertFirstButtonReturn
}

pub(crate) fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

pub(crate) fn add(parent: &NSView, child: &NSView) {
    parent.addSubview(child);
}

pub(crate) fn label(mtm: MainThreadMarker, text: &str, frame: NSRect) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    label
}

pub(crate) fn secondary(mtm: MainThreadMarker, text: &str, frame: NSRect) -> Retained<NSTextField> {
    let label = label(mtm, text, frame);
    label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    label
}

pub(crate) fn button(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: Sel,
    frame: NSRect,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setFrame(frame);
    button
}

fn checkbox(
    mtm: MainThreadMarker,
    title: &str,
    on: bool,
    target: &SettingsTarget,
    action: Sel,
    frame: NSRect,
) -> Retained<NSButton> {
    let checkbox = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    };
    checkbox.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    checkbox.setFrame(frame);
    checkbox
}
