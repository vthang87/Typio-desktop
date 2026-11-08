//! Quick Search (⌃⌥Space): a search box that takes keyboard input without taking focus
//! from the app the user is typing in, with results in the suggestion popup below it.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSFocusRingType, NSFont, NSPanel, NSScreen, NSTextField, NSWindowButton,
    NSWindowCollectionBehavior, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use typio_core::RankedSuggestion;
use typio_platform::Rect;

use crate::i18n::{tr, T};
use crate::popup::Popup;

const WIDTH: f64 = 520.0;
const HEIGHT: f64 = 52.0;
const LEVEL: isize = 101;

pub struct Palette {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    field: Retained<NSTextField>,
    results: Popup,
    items: Vec<RankedSuggestion>,
}

impl Palette {
    pub fn new(mtm: MainThreadMarker) -> Self {
        // A titled, non-activating panel can become key while the app the user types in
        // stays active, so its focused field is still there to insert into.
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::NonactivatingPanel
                | NSWindowStyleMask::FullSizeContentView,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setTitlebarAppearsTransparent(true);
        panel.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        for button in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(button) = panel.standardWindowButton(button) {
                button.setHidden(true);
            }
        }
        panel.setLevel(LEVEL);
        panel.setFloatingPanel(true);
        panel.setHidesOnDeactivate(false);
        panel.setMovableByWindowBackground(true);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        unsafe { panel.setReleasedWhenClosed(false) };

        let field = NSTextField::textFieldWithString(&NSString::new(), mtm);
        field.setPlaceholderString(Some(&NSString::from_str(tr(T::SearchPlaceholder))));
        field.setFont(Some(&NSFont::systemFontOfSize(18.0)));
        field.setBezeled(false);
        field.setDrawsBackground(false);
        field.setFocusRingType(NSFocusRingType::None);
        field.setFrame(NSRect::new(
            NSPoint::new(16.0, 12.0),
            NSSize::new(WIDTH - 32.0, 28.0),
        ));
        if let Some(content) = panel.contentView() {
            content.addSubview(&field);
        }

        Self {
            mtm,
            panel,
            field,
            results: Popup::new(mtm),
            items: Vec::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.panel.isVisible()
    }

    /// Shows an empty search box near the top of the screen the user is on.
    pub fn open(&mut self) {
        self.field.setStringValue(&NSString::new());
        self.field
            .setPlaceholderString(Some(&NSString::from_str(tr(T::SearchPlaceholder))));
        self.items.clear();
        self.results.hide();
        if let Some(screen) = NSScreen::mainScreen(self.mtm) {
            let area = screen.visibleFrame();
            let origin = NSPoint::new(
                area.origin.x + (area.size.width - WIDTH) / 2.0,
                area.origin.y + area.size.height * 0.7,
            );
            self.panel.setFrameOrigin(origin);
        }
        self.panel.makeKeyAndOrderFront(None);
        self.panel.makeFirstResponder(Some(&self.field));
    }

    pub fn close(&mut self) {
        self.results.hide();
        self.items.clear();
        self.panel.orderOut(None);
    }

    pub fn query(&self) -> String {
        self.field.stringValue().to_string()
    }

    /// Lists `items` under the search box with the first one highlighted.
    pub fn show(&mut self, items: Vec<RankedSuggestion>) {
        self.items = items;
        if self.items.is_empty() {
            self.results.hide();
            return;
        }
        self.results.show(&self.items, self.frame());
        self.results.select(Some(0));
    }

    pub fn move_selection(&mut self, down: bool) {
        let count = self.items.len();
        if count == 0 {
            return;
        }
        let next = match (self.results.selected(), down) {
            (None, _) => 0,
            (Some(index), true) => (index + 1) % count,
            (Some(index), false) => (index + count - 1) % count,
        };
        self.results.select(Some(next));
    }

    pub fn selected(&self) -> Option<&RankedSuggestion> {
        self.items.get(self.results.selected()?)
    }

    pub fn item_at(&self, x: f64, y: f64) -> Option<&RankedSuggestion> {
        self.items.get(self.results.row_at(x, y)?)
    }

    /// Whether (`x`, `y`) in top-left screen points is on the search box.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        self.frame().contains(x, y)
    }

    /// The panel's frame in top-left screen coordinates.
    fn frame(&self) -> Rect {
        let frame = self.panel.frame();
        let primary_height = NSScreen::screens(self.mtm)
            .firstObject()
            .map_or(0.0, |screen| screen.frame().size.height);
        Rect {
            x: frame.origin.x,
            y: primary_height - frame.origin.y - frame.size.height,
            width: frame.size.width,
            height: frame.size.height,
        }
    }
}
