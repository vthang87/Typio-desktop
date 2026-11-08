//! The suggestion list: a borderless panel that never takes focus from the field.
//! Mouse clicks reach it through the event tap, so the panel itself ignores the mouse.

use crate::i18n::{tr, T};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBox, NSBoxType, NSColor, NSFont, NSPanel, NSScreen, NSTextField, NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use typio_core::{RankedSuggestion, SuggestionKind};
use typio_platform::Rect;

const ROW_HEIGHT: f64 = 26.0;
const PADDING: f64 = 5.0;
const GAP: f64 = 4.0;
/// Space between a row's edges and its text, on both sides.
const INSET: f64 = 10.0;
const MIN_WIDTH: f64 = 240.0;
const MAX_WIDTH: f64 = 520.0;
/// `NSPopUpMenuWindowLevel`: above normal and floating windows.
const POPUP_LEVEL: isize = 101;

pub struct Popup {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    rows: Vec<Retained<NSBox>>,
    labels: Vec<(Retained<NSTextField>, Retained<NSTextField>)>,
    selected: Option<usize>,
    /// Screen frame in top-left coordinates, for hit-testing clicks from the event tap.
    frame: Rect,
}

impl Popup {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(MIN_WIDTH, ROW_HEIGHT)),
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setLevel(POPUP_LEVEL);
        panel.setFloatingPanel(true);
        panel.setHidesOnDeactivate(false);
        panel.setHasShadow(true);
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setIgnoresMouseEvents(true);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Transient,
        );
        unsafe { panel.setReleasedWhenClosed(false) };

        Self {
            mtm,
            panel,
            rows: Vec::new(),
            labels: Vec::new(),
            selected: None,
            frame: Rect::default(),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.panel.isVisible()
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected.filter(|_| self.is_visible())
    }

    pub fn hide(&mut self) {
        self.panel.orderOut(None);
        self.selected = None;
    }

    /// The row under a screen point (top-left coordinates), if the point is in the list.
    pub fn row_at(&self, x: f64, y: f64) -> Option<usize> {
        if !self.is_visible() || !self.frame.contains(x, y) {
            return None;
        }
        let row = ((y - self.frame.y - PADDING) / ROW_HEIGHT).floor();
        (row >= 0.0 && (row as usize) < self.rows.len()).then_some(row as usize)
    }

    /// Shows `items` below `anchor` (the caret or the field), flipping above when needed.
    pub fn show(&mut self, items: &[RankedSuggestion], anchor: Rect) {
        let rows: Vec<(String, String)> = items
            .iter()
            .map(|item| {
                let hint = match (&item.trigger, item.kind) {
                    (Some(trigger), _) => format!("{trigger} →"),
                    (None, SuggestionKind::Initials) => tr(T::InitialsHint).into(),
                    _ => String::new(),
                };
                (item.value.clone(), hint)
            })
            .collect();
        self.render(&rows, anchor);
    }

    /// Shows a short notice, such as "Typio on".
    pub fn show_message(&mut self, text: &str, anchor: Rect) {
        self.render(&[(text.to_string(), String::new())], anchor);
    }

    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index;
        for (position, (row, (label, hint))) in self.rows.iter().zip(&self.labels).enumerate() {
            let active = Some(position) == index;
            let fill = if active {
                NSColor::selectedContentBackgroundColor()
            } else {
                NSColor::clearColor()
            };
            row.setFillColor(&fill);
            let text_color = if active {
                NSColor::alternateSelectedControlTextColor()
            } else {
                NSColor::labelColor()
            };
            label.setTextColor(Some(&text_color));
            let hint_color = if active {
                text_color
            } else {
                NSColor::secondaryLabelColor()
            };
            hint.setTextColor(Some(&hint_color));
        }
    }

    fn render(&mut self, rows: &[(String, String)], anchor: Rect) {
        let mtm = self.mtm;
        let font = NSFont::systemFontOfSize(13.0);
        let hint_font = NSFont::systemFontOfSize(12.0);

        let mut labels = Vec::new();
        let mut width = MIN_WIDTH;
        for (text, hint) in rows {
            let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
            label.setFont(Some(&font));
            let hint_label = NSTextField::labelWithString(&NSString::from_str(hint), mtm);
            hint_label.setFont(Some(&hint_font));
            width = width.max(
                label.fittingSize().width
                    + hint_label.fittingSize().width
                    + INSET * 2.0
                    + 16.0
                    + PADDING * 2.0,
            );
            labels.push((label, hint_label));
        }
        let width = width.min(MAX_WIDTH);
        let height = rows.len() as f64 * ROW_HEIGHT + PADDING * 2.0;

        let background = NSBox::new(mtm);
        background.setBoxType(NSBoxType::Custom);
        background.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
        background.setFillColor(&NSColor::windowBackgroundColor());
        background.setBorderColor(&NSColor::separatorColor());
        background.setBorderWidth(1.0);
        background.setCornerRadius(8.0);
        background.setContentViewMargins(NSSize::new(0.0, 0.0));
        background.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, height),
        ));
        let content: Retained<NSView> = background.contentView().expect("NSBox has a content view");

        self.rows.clear();
        for (index, (label, hint)) in labels.iter().enumerate() {
            let y = height - PADDING - (index as f64 + 1.0) * ROW_HEIGHT;
            let row = NSBox::new(mtm);
            row.setBoxType(NSBoxType::Custom);
            row.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
            row.setBorderWidth(0.0);
            row.setContentViewMargins(NSSize::new(0.0, 0.0));
            row.setCornerRadius(5.0);
            row.setFillColor(&NSColor::clearColor());
            row.setFrame(NSRect::new(
                NSPoint::new(PADDING, y),
                NSSize::new(width - PADDING * 2.0, ROW_HEIGHT),
            ));

            let row_width = width - PADDING * 2.0;
            let hint_size = hint.fittingSize();
            let label_height = label.fittingSize().height;
            let label_y = ((ROW_HEIGHT - label_height) / 2.0).round();
            label.setFrame(NSRect::new(
                NSPoint::new(INSET, label_y),
                NSSize::new(
                    row_width - INSET * 2.0 - hint_size.width - 16.0,
                    label_height,
                ),
            ));
            label.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingTail);
            // Put the hint on the label's baseline: both draw their baseline one descender
            // above the bottom of their frame.
            let hint_y = label_y + font.descender().abs() - hint_font.descender().abs();
            hint.setFrame(NSRect::new(
                NSPoint::new(row_width - INSET - hint_size.width, hint_y.round()),
                hint_size,
            ));
            let row_content: Retained<NSView> =
                row.contentView().expect("NSBox has a content view");
            row_content.addSubview(label);
            row_content.addSubview(hint);
            content.addSubview(&row);
            self.rows.push(row);
        }
        self.labels = labels;

        let (origin, frame) = place(mtm, anchor, width, height);
        self.panel.setContentView(Some(&background));
        self.panel
            .setFrame_display(NSRect::new(origin, NSSize::new(width, height)), true);
        self.frame = frame;
        self.select(None);
        self.panel.orderFrontRegardless();
    }
}

/// Where the panel goes in AppKit coordinates (bottom-left origin), and its frame in
/// top-left screen coordinates.
fn place(mtm: MainThreadMarker, anchor: Rect, width: f64, height: f64) -> (NSPoint, Rect) {
    let screens = NSScreen::screens(mtm);
    let primary_height = screens
        .firstObject()
        .map_or(0.0, |screen| screen.frame().size.height);
    let flip = |top: f64| primary_height - top;

    // The visible area of the screen holding the anchor, in top-left coordinates.
    let visible = screens
        .iter()
        .map(|screen| {
            let frame = screen.visibleFrame();
            Rect {
                x: frame.origin.x,
                y: flip(frame.origin.y + frame.size.height),
                width: frame.size.width,
                height: frame.size.height,
            }
        })
        .find(|area| area.contains(anchor.x, anchor.y + anchor.height / 2.0))
        .unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: f64::MAX,
            height: f64::MAX,
        });

    let below = anchor.y + anchor.height + GAP;
    let top = if below + height > visible.y + visible.height && anchor.y - GAP - height > visible.y
    {
        anchor.y - GAP - height
    } else {
        below
    };
    // Line the row text up with the text being typed.
    let x = (anchor.x - PADDING - INSET)
        .min(visible.x + visible.width - width)
        .max(visible.x);
    let frame = Rect {
        x,
        y: top,
        width,
        height,
    };
    (NSPoint::new(x, flip(top + height)), frame)
}
