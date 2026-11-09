//! The Settings window: the on/off switch, the trigger character, Accessibility access and
//! the datasets. Its content is rebuilt from `UiState` whenever anything changes.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSBackingStoreType, NSBorderType, NSBox,
    NSBoxType, NSButton, NSColor, NSControlSize, NSControlStateValueOff, NSControlStateValueOn,
    NSFont, NSLineBreakMode, NSPopUpButton, NSScrollView, NSTextAlignment, NSTextField, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::controller::{with_app, UiState};
use crate::menu;
use crate::store::TRIGGERS;

const WIDTH: f64 = 520.0;
const HEIGHT: f64 = 474.0;
const MARGIN: f64 = 20.0;
const LIST_TOP: f64 = 184.0;
const LIST_HEIGHT: f64 = 230.0;
const ROW_HEIGHT: f64 = 32.0;

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

struct Settings {
    window: Retained<NSWindow>,
    target: Retained<SettingsTarget>,
}

define_class!(
    /// A view whose y axis runs down, so layout reads top to bottom.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "TypioFlippedView"]
    struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }

    unsafe impl NSObjectProtocol for FlippedView {}
);

impl FlippedView {
    fn new(mtm: MainThreadMarker, size: NSSize) -> Retained<Self> {
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
    let inner = WIDTH - MARGIN * 2.0;

    add(&content, &heading(mtm, "General", MARGIN, 18.0));
    let enable = checkbox(
        mtm,
        "Enable Typio in all text fields   ⌃⌥T",
        state.enabled,
        target,
        sel!(toggleEnabled:),
        rect(MARGIN, 44.0, inner, 20.0),
    );
    add(&content, &enable);

    add(
        &content,
        &label(mtm, "Trigger character", rect(MARGIN, 79.0, 130.0, 18.0)),
    );
    let trigger = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(MARGIN + 130.0, 74.0, 64.0, 26.0),
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
    add(&content, &trigger);
    add(
        &content,
        &secondary(
            mtm,
            &format!("Type {}xtt to see suggestions for xtt", state.trigger),
            rect(MARGIN + 204.0, 80.0, inner - 204.0, 16.0),
        ),
    );

    let access = if state.trusted {
        "Accessibility access: granted"
    } else {
        "Accessibility access: not granted"
    };
    add(
        &content,
        &label(mtm, access, rect(MARGIN, 114.0, 240.0, 18.0)),
    );
    if !state.trusted {
        add(
            &content,
            &button(
                mtm,
                "Open Accessibility Settings…",
                target,
                sel!(openAccessibilitySettings:),
                rect(WIDTH - MARGIN - 230.0, 108.0, 230.0, 28.0),
            ),
        );
    }

    let separator = NSBox::new(mtm);
    separator.setBoxType(NSBoxType::Separator);
    separator.setFrame(rect(MARGIN, 146.0, inner, 1.0));
    add(&content, &separator);

    add(&content, &heading(mtm, "Datasets", MARGIN, 158.0));
    let hint = secondary(
        mtm,
        "Only checked datasets give suggestions",
        rect(WIDTH - MARGIN - 260.0, 161.0, 260.0, 16.0),
    );
    hint.setAlignment(NSTextAlignment::Right);
    add(&content, &hint);
    add(&content, &dataset_list(mtm, state, target, inner));

    let buttons_top = LIST_TOP + LIST_HEIGHT + 12.0;
    add(
        &content,
        &button(
            mtm,
            "Import…",
            target,
            sel!(importDataset:),
            rect(MARGIN - 6.0, buttons_top, 110.0, 32.0),
        ),
    );
    add(
        &content,
        &button(
            mtm,
            "Open Data Folder",
            target,
            sel!(openDataFolder:),
            rect(MARGIN + 104.0, buttons_top, 160.0, 32.0),
        ),
    );

    settings.window.setContentView(Some(&content));
}

fn dataset_list(
    mtm: MainThreadMarker,
    state: &UiState,
    target: &SettingsTarget,
    width: f64,
) -> Retained<NSScrollView> {
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        rect(MARGIN, LIST_TOP, width, LIST_HEIGHT),
    );
    scroll.setBorderType(NSBorderType::BezelBorder);
    scroll.setHasVerticalScroller(true);
    let width = scroll.contentSize().width;
    let height = (state.datasets.len() as f64 * ROW_HEIGHT).max(scroll.contentSize().height);
    let list = FlippedView::new(mtm, NSSize::new(width, height));

    if state.datasets.is_empty() {
        add(
            &list,
            &secondary(
                mtm,
                "No datasets yet. Click Import… to add a TXT, CSV or JSON file.",
                rect(12.0, 12.0, width - 24.0, 16.0),
            ),
        );
    }
    for (index, dataset) in state.datasets.iter().enumerate() {
        let top = index as f64 * ROW_HEIGHT;
        let enable = checkbox(
            mtm,
            &dataset.name,
            dataset.enabled,
            target,
            sel!(toggleDataset:),
            rect(12.0, top + 6.0, width - 250.0, 20.0),
        );
        enable.setTag(index as isize);
        enable.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        add(&list, &enable);

        let detail = secondary(
            mtm,
            &dataset.detail,
            rect(width - 230.0, top + 8.0, 130.0, 16.0),
        );
        detail.setAlignment(NSTextAlignment::Right);
        add(&list, &detail);

        let delete = button(
            mtm,
            "Delete",
            target,
            sel!(deleteDataset:),
            rect(width - 88.0, top + 4.0, 76.0, 24.0),
        );
        delete.setControlSize(NSControlSize::Small);
        delete.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        delete.setTag(index as isize);
        add(&list, &delete);
    }
    scroll.setDocumentView(Some(&list));
    scroll
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

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn add(parent: &NSView, child: &NSView) {
    parent.addSubview(child);
}

fn label(mtm: MainThreadMarker, text: &str, frame: NSRect) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    label
}

fn heading(mtm: MainThreadMarker, text: &str, x: f64, y: f64) -> Retained<NSTextField> {
    let heading = label(mtm, text, rect(x, y, 200.0, 18.0));
    heading.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    heading
}

fn secondary(mtm: MainThreadMarker, text: &str, frame: NSRect) -> Retained<NSTextField> {
    let label = label(mtm, text, frame);
    label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    label
}

fn button(
    mtm: MainThreadMarker,
    title: &str,
    target: &SettingsTarget,
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
