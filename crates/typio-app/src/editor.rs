//! The dataset editor: a name and the dataset as text, one shortcut (`trigger: text`) or
//! value per line, the same format Typio imports.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBorderType, NSEventModifierFlags, NSFont, NSPopUpButton,
    NSTextField, NSTextView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use typio_core::import::format_dataset_export;
use typio_core::DatasetKind;

use crate::controller::with_app;
use crate::menu;
use crate::settings::{add, button, label, rect, secondary, FlippedView};

const WIDTH: f64 = 560.0;
const HEIGHT: f64 = 480.0;
const MARGIN: f64 = 16.0;
const KINDS: [(DatasetKind, &str); 2] = [
    (DatasetKind::Shortcuts, "Shortcuts"),
    (DatasetKind::Values, "Values"),
];

thread_local! {
    static EDITOR: RefCell<Option<Editor>> = const { RefCell::new(None) };
}

struct Editor {
    window: Retained<NSWindow>,
    _target: Retained<EditorTarget>,
    name: Retained<NSTextField>,
    kind: Retained<NSPopUpButton>,
    text: Retained<NSTextView>,
    /// The dataset being edited; `None` for a new one.
    dataset: Option<String>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and EditorTarget does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TypioEditorTarget"]
    struct EditorTarget;

    impl EditorTarget {
        #[unsafe(method(save:))]
        fn save(&self, _sender: Option<&AnyObject>) {
            save(self.mtm());
        }

        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: Option<&AnyObject>) {
            close_later();
        }
    }

    unsafe impl NSObjectProtocol for EditorTarget {}
);

impl EditorTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Opens the editor for the dataset at `index` in the store, or for a new dataset.
pub fn open(mtm: MainThreadMarker, index: Option<usize>) {
    let dataset = match index {
        Some(index) => {
            let Some(Some(found)) = with_app(|app| {
                app.store.datasets.get(index).map(|dataset| {
                    (
                        dataset.id.clone(),
                        dataset.name.clone(),
                        dataset.kind(),
                        format_dataset_export(dataset),
                    )
                })
            }) else {
                return;
            };
            Some(found)
        }
        None => None,
    };
    close();

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    let title = match &dataset {
        Some((_, name, _, _)) => format!("Edit “{name}”"),
        None => "New Dataset".to_string(),
    };
    window.setTitle(&NSString::from_str(&title));

    let target = EditorTarget::new(mtm);
    let content = FlippedView::new(mtm, NSSize::new(WIDTH, HEIGHT));
    let inner = WIDTH - MARGIN * 2.0;

    add(
        &content,
        &label(mtm, "Name", rect(MARGIN, 19.0, 70.0, 18.0)),
    );
    let name = NSTextField::textFieldWithString(
        &NSString::from_str(dataset.as_ref().map_or("", |(_, name, _, _)| name)),
        mtm,
    );
    name.setFrame(rect(MARGIN + 70.0, 16.0, inner - 70.0, 24.0));
    add(&content, &name);

    add(
        &content,
        &label(mtm, "Type", rect(MARGIN, 55.0, 70.0, 18.0)),
    );
    let kind = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(MARGIN + 66.0, 50.0, 140.0, 26.0),
        false,
    );
    for (_, title) in KINDS {
        kind.addItemWithTitle(&NSString::from_str(title));
    }
    if let Some((_, _, existing, _)) = &dataset {
        let index = KINDS
            .iter()
            .position(|(kind, _)| kind == existing)
            .unwrap_or(0);
        kind.selectItemAtIndex(index as isize);
        // Changing the type would reinterpret every line.
        kind.setEnabled(false);
    }
    add(&content, &kind);

    let help = secondary(
        mtm,
        "Shortcuts: one per line as trigger: text, for example xtt: Xã Tam Tiến.\nValues: one value per line.",
        rect(MARGIN, 86.0, inner, 34.0),
    );
    add(&content, &help);

    let scroll = NSTextView::scrollableTextView(mtm);
    scroll.setFrame(rect(MARGIN, 126.0, inner, HEIGHT - 126.0 - 60.0));
    scroll.setBorderType(NSBorderType::BezelBorder);
    let Some(text) = scroll
        .documentView()
        .and_then(|view| view.downcast::<NSTextView>().ok())
    else {
        return;
    };
    text.setRichText(false);
    text.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(13.0, 0.0)));
    text.setAutomaticQuoteSubstitutionEnabled(false);
    text.setAutomaticDashSubstitutionEnabled(false);
    text.setAutomaticTextReplacementEnabled(false);
    text.setAutomaticSpellingCorrectionEnabled(false);
    text.setAllowsUndo(true);
    text.setString(&NSString::from_str(
        dataset.as_ref().map_or("", |(_, _, _, body)| body),
    ));
    add(&content, &scroll);

    let buttons_top = HEIGHT - 46.0;
    add(
        &content,
        &button(
            mtm,
            "Cancel",
            &target,
            sel!(cancel:),
            rect(WIDTH - MARGIN - 196.0, buttons_top, 96.0, 32.0),
        ),
    );
    let save = button(
        mtm,
        "Save",
        &target,
        sel!(save:),
        rect(WIDTH - MARGIN - 96.0, buttons_top, 96.0, 32.0),
    );
    save.setKeyEquivalent(&NSString::from_str("s"));
    save.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    add(&content, &save);
    add(
        &content,
        &secondary(
            mtm,
            "⌘S saves",
            rect(MARGIN, buttons_top + 9.0, 200.0, 16.0),
        ),
    );

    window.setContentView(Some(&content));
    window.center();
    NSApplication::sharedApplication(mtm).activate();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&text));

    EDITOR.with_borrow_mut(|slot| {
        *slot = Some(Editor {
            window,
            _target: target,
            name,
            kind,
            text,
            dataset: dataset.map(|(id, _, _, _)| id),
        });
    });
}

fn save(mtm: MainThreadMarker) {
    let Some((dataset, name, kind, text)) = EDITOR.with_borrow(|slot| {
        let editor = slot.as_ref()?;
        let kind = usize::try_from(editor.kind.indexOfSelectedItem())
            .ok()
            .and_then(|index| KINDS.get(index))
            .map_or(DatasetKind::Shortcuts, |(kind, _)| *kind);
        Some((
            editor.dataset.clone(),
            editor.name.stringValue().to_string(),
            kind,
            editor.text.string().to_string(),
        ))
    }) else {
        return;
    };
    let result = with_app(|app| app.save_dataset(dataset.as_deref(), &name, kind, &text));
    match result {
        Some(Ok(warnings)) => {
            close_later();
            if !warnings.is_empty() {
                let mut shown: Vec<String> = warnings.iter().take(10).cloned().collect();
                if warnings.len() > shown.len() {
                    shown.push(format!("…and {} more.", warnings.len() - shown.len()));
                }
                menu::alert(
                    mtm,
                    "Saved. Some triggers are used twice",
                    &shown.join("\n"),
                );
            }
        }
        Some(Err(reason)) => menu::alert(mtm, "Typio could not save this dataset", &reason),
        None => menu::alert(mtm, "Typio is busy", "Try saving again."),
    }
}

/// Closes the editor after the current action returns; the action's target is part of it.
fn close_later() {
    let _ = dispatch2::DispatchQueue::main().after(dispatch2::DispatchTime::NOW, close);
}

fn close() {
    let editor = EDITOR.with_borrow_mut(Option::take);
    if let Some(editor) = editor {
        editor.window.close();
    }
}
