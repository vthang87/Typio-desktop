//! Ties the keyboard hook, the focused field, the store and the popup together on Windows.
//!
//! The hook runs on its own thread and decides at once, from a small shared state, which
//! keys to hold back; everything else (UI Automation, typing, the popup) runs on the main
//! thread from the commands the hook queues.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use typio_core::engine::{prefixed_word_at, Suggester, WordRange};
use typio_core::sensitive::is_sensitive_label;
use typio_core::{DatasetKind, RankedSuggestion};
use typio_platform::{
    focused_field, press_backspace, Disposition, EventTap, Field, InputEvent, Key, Rect,
};

use super::popup::Popup;
use crate::i18n::{tr, trf, Language, T};
use crate::store::{AppMode, AppRef, Store};

/// Lets the app finish handling the key before Typio reads the field.
const REFRESH_DELAY: Duration = Duration::from_millis(40);
const MESSAGE_DURATION: Duration = Duration::from_millis(1600);

/// What the main thread asks of the controller.
#[derive(Debug, Clone, Copy)]
pub enum Command {
    Refresh,
    Hide,
    Select(usize),
    Insert(usize),
    Dismiss,
    Undo,
    Toggle,
    Mouse(f64, f64),
}

/// What the hook needs to decide on a key without waiting for the main thread.
struct HookState {
    /// The popup lists suggestions (not a notice).
    listing: bool,
    count: usize,
    selected: Option<usize>,
    /// The next plain Backspace undoes the last expansion.
    undo_armed: bool,
}

static HOOK: Mutex<HookState> = Mutex::new(HookState {
    listing: false,
    count: 0,
    selected: None,
    undo_armed: false,
});
static QUEUE: Mutex<VecDeque<Command>> = Mutex::new(VecDeque::new());

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn hook_state() -> std::sync::MutexGuard<'static, HookState> {
    HOOK.lock().unwrap_or_else(|error| error.into_inner())
}

fn queue() -> std::sync::MutexGuard<'static, VecDeque<Command>> {
    QUEUE.lock().unwrap_or_else(|error| error.into_inner())
}

/// Runs on the hook thread for every key and mouse press.
fn decide(event: InputEvent) -> Disposition {
    let mut commands = Vec::new();
    let disposition = {
        let mut state = hook_state();
        decide_with(&mut state, event, &mut commands)
    };
    if !commands.is_empty() {
        queue().extend(commands);
        super::wake();
    }
    disposition
}

fn decide_with(state: &mut HookState, event: InputEvent, send: &mut Vec<Command>) -> Disposition {
    let undo_armed = std::mem::take(&mut state.undo_armed);
    let InputEvent::KeyDown { key, modifiers } = event else {
        if let InputEvent::MouseDown { x, y } = event {
            send.push(Command::Mouse(x, y));
        }
        return Disposition::Pass;
    };
    let plain = !modifiers.control && !modifiers.option && !modifiers.command;

    // Only the very next key can undo an expansion.
    if key == Key::Backspace && plain && undo_armed && !state.listing {
        send.push(Command::Undo);
        return Disposition::Swallow;
    }
    if key == Key::T
        && modifiers.control
        && modifiers.option
        && !modifiers.command
        && !modifiers.shift
    {
        send.push(Command::Toggle);
        return Disposition::Swallow;
    }
    if !state.listing {
        send.push(Command::Refresh);
        return Disposition::Pass;
    }
    match key {
        Key::Down | Key::Up if plain && state.count > 0 => {
            let count = state.count;
            let next = match (state.selected, key) {
                (None, Key::Down) => 0,
                (None, _) => count - 1,
                (Some(index), Key::Down) => (index + 1) % count,
                (Some(index), _) => (index + count - 1) % count,
            };
            state.selected = Some(next);
            send.push(Command::Select(next));
            Disposition::Swallow
        }
        // Enter and Tab only insert an item the user highlighted; otherwise they keep
        // their usual meaning.
        Key::Return | Key::Tab if plain => match state.selected {
            Some(index) => {
                state.listing = false;
                send.push(Command::Insert(index));
                Disposition::Swallow
            }
            None => {
                state.listing = false;
                send.push(Command::Hide);
                Disposition::Pass
            }
        },
        Key::Escape => {
            state.listing = false;
            send.push(Command::Dismiss);
            Disposition::Swallow
        }
        _ => {
            if !plain {
                state.listing = false;
                send.push(Command::Hide);
            }
            send.push(Command::Refresh);
            Disposition::Pass
        }
    }
}

/// Runs the queued commands. When the app is busy (a UI Automation call can let messages
/// through), they wait and run a moment later.
pub fn drain() {
    loop {
        let Some(command) = queue().pop_front() else {
            return;
        };
        if with_app(|app| app.apply(command)).is_none() {
            queue().push_front(command);
            super::set_timer(super::TIMER_DRAIN, Duration::from_millis(10));
            return;
        }
    }
}

/// Queues `command` for the main thread, as the hook does.
pub fn post(command: Command) {
    queue().push_back(command);
    super::wake();
}

pub fn install(store: Store) {
    let app = App {
        store,
        popup: Popup::new(),
        tap: None,
        field: None,
        word: None,
        results: Vec::new(),
        dismissed: None,
        suggester: None,
        store_version: 0,
        last_expansion: None,
        last_app: None,
    };
    APP.with(|cell| *cell.borrow_mut() = Some(app));
}

/// Runs `action` on the app; `None` while it is already busy.
pub fn with_app<R>(action: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(action))
}

/// The last expansion, which Backspace right after it undoes.
struct Expansion {
    pid: i32,
    /// UTF-16 offset where the inserted text starts.
    start: usize,
    inserted: String,
    /// What it replaced, such as `:xtt`.
    original: String,
}

pub struct App {
    pub store: Store,
    popup: Popup,
    tap: Option<EventTap>,
    field: Option<Field>,
    word: Option<WordRange>,
    results: Vec<RankedSuggestion>,
    /// The field text when the user pressed Escape; no suggestions until it changes.
    dismissed: Option<String>,
    suggester: Option<(u64, Suggester)>,
    store_version: u64,
    last_expansion: Option<Expansion>,
    /// The last app Typio saw a text field in, for the Skip item in the tray menu (the app
    /// in front is the taskbar while the menu is open).
    pub last_app: Option<(String, String)>,
}

/// What the tray menu and the Settings window show.
#[derive(Debug, Clone, PartialEq)]
pub struct UiState {
    pub enabled: bool,
    pub trigger: char,
    pub language: Language,
    /// Name, detail (such as "99 shortcuts") and whether it is enabled.
    pub datasets: Vec<(String, String, bool)>,
    pub app_mode: AppMode,
    /// Skipped apps in `AppMode::All`, selected apps in `AppMode::Selected`.
    pub apps: Vec<AppRef>,
    /// The last app typed in: name and whether it is in the app list.
    pub app: Option<(String, bool)>,
}

impl App {
    pub fn start_tap(&mut self) {
        match EventTap::install(decide) {
            Ok(tap) => {
                info!("Keyboard hook installed");
                self.tap = Some(tap);
            }
            Err(error) => info!("Keyboard hook failed: {error}"),
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Refresh => super::set_timer(super::TIMER_REFRESH, REFRESH_DELAY),
            Command::Hide => self.hide_popup(),
            Command::Select(index) => self.popup.select(Some(index)),
            Command::Insert(index) => self.insert(index),
            Command::Dismiss => {
                self.dismissed = self.field.as_ref().map(|field| field.info.value.clone());
                self.hide_popup();
            }
            Command::Undo => self.undo(),
            Command::Toggle => self.toggle_enabled(),
            Command::Mouse(x, y) => {
                if !self.popup.contains(x, y) {
                    self.hide_popup();
                }
            }
        }
    }

    fn hide_popup(&mut self) {
        self.popup.hide();
        self.results.clear();
        self.sync_hook();
    }

    /// Tells the hook what the popup shows now.
    fn sync_hook(&self) {
        let mut state = hook_state();
        state.listing = !self.results.is_empty() && self.popup.is_visible();
        state.count = self.results.len();
        state.selected = self.popup.selected();
    }

    pub fn store_changed(&mut self) {
        self.store_version += 1;
        if let Err(error) = self.store.save() {
            info!("Could not save data: {error}");
        }
        super::refresh_ui();
    }

    pub fn refresh(&mut self) {
        self.results.clear();
        self.field = None;
        let field = match focused_field() {
            Ok(field) => field,
            Err(error) => {
                debug!("refresh: {error:?}");
                self.hide_popup();
                return;
            }
        };
        let info = &field.info;
        self.last_app = Some((info.app_id.clone(), info.app_name.clone()));
        if !self.store.enabled || is_blocked(&field) || !self.store.runs_in(&info.app_id) {
            debug!("refresh: {} {} is off or blocked", info.app_id, info.role);
            self.hide_popup();
            return;
        }
        if self.dismissed.as_deref() == Some(info.value.as_str()) {
            self.hide_popup();
            self.field = Some(field);
            return;
        }
        self.dismissed = None;

        // Suggestions only come up for a word typed after the trigger, such as `:xtt`.
        let word = info
            .caret
            .and_then(|caret| prefixed_word_at(&info.value, caret, self.store.trigger));
        let results = match &word {
            Some(word) => self.suggester().suggest(&word.text, None),
            None => Vec::new(),
        };
        self.word = word;
        self.results = results;
        let info = &field.info;
        debug!(
            "refresh {} {} caret={:?} length={} word={} results={} caret_rect={} field_rect={}",
            info.app_id,
            info.role,
            info.caret,
            info.value.encode_utf16().count(),
            self.word.is_some(),
            self.results.len(),
            info.caret_rect.is_some(),
            info.field_rect.is_some()
        );
        match (self.results.is_empty(), info.caret_rect.or(info.field_rect)) {
            (false, Some(anchor)) => self.popup.show(&self.results, anchor),
            _ => {
                self.results.clear();
                self.popup.hide();
            }
        }
        self.field = Some(field);
        self.sync_hook();
    }

    /// The index of the enabled datasets, rebuilt after the store changes.
    fn suggester(&mut self) -> &Suggester {
        if !matches!(&self.suggester, Some((version, _)) if *version == self.store_version) {
            let suggester = Suggester::new(self.store.enabled_datasets());
            self.suggester = Some((self.store_version, suggester));
        }
        &self.suggester.as_ref().expect("just set").1
    }

    fn insert(&mut self, index: usize) {
        let choice = self.results.get(index).cloned();
        self.hide_popup();
        let (Some(field), Some(choice), Some(word)) = (self.field.take(), choice, self.word.take())
        else {
            return;
        };
        // The trigger and the word after it become the chosen text.
        let original = utf16_slice(&field.info.value, word.start, word.end);
        if field
            .replace(word.start, word.end - word.start, &choice.value)
            .is_err()
        {
            self.show_message(tr(T::CannotTypeField), anchor_of(&field));
            return;
        }
        self.last_expansion = Some(Expansion {
            pid: field.info.pid,
            start: word.start,
            inserted: choice.value,
            original,
        });
        hook_state().undo_armed = true;
    }

    /// Puts back what the last expansion replaced when the inserted text still sits right
    /// before the caret; otherwise presses the Backspace the hook held back.
    fn undo(&mut self) {
        let restored = self.last_expansion.take().is_some_and(|expansion| {
            let Ok(field) = focused_field() else {
                return false;
            };
            let length = expansion.inserted.encode_utf16().count();
            let end = expansion.start + length;
            field.info.pid == expansion.pid
                && field.info.caret == Some(end)
                && utf16_slice(&field.info.value, expansion.start, end) == expansion.inserted
                && field
                    .replace(expansion.start, length, &expansion.original)
                    .is_ok()
        });
        if !restored && press_backspace().is_err() {
            info!("Could not pass on a Backspace");
        }
    }

    pub fn ui_state(&self) -> UiState {
        let datasets = self
            .store
            .datasets
            .iter()
            .map(|dataset| {
                let detail = match dataset.kind() {
                    DatasetKind::Shortcuts => trf(
                        T::ShortcutCount,
                        &[(
                            "count",
                            &dataset.shortcuts.as_ref().map_or(0, Vec::len).to_string(),
                        )],
                    ),
                    DatasetKind::Values => trf(
                        T::ValueCount,
                        &[("count", &dataset.values.len().to_string())],
                    ),
                };
                (
                    dataset.name.clone(),
                    detail,
                    self.store.is_dataset_enabled(&dataset.id),
                )
            })
            .collect();
        UiState {
            enabled: self.store.enabled,
            trigger: self.store.trigger,
            language: self.store.language,
            datasets,
            app_mode: self.store.app_mode,
            apps: self.store.app_list().to_vec(),
            app: self
                .last_app
                .as_ref()
                .map(|(id, name)| (name.clone(), self.store.is_listed(id))),
        }
    }

    /// `index` is the dataset's position in the store.
    pub fn toggle_dataset(&mut self, index: usize) {
        let Some(id) = self
            .store
            .datasets
            .get(index)
            .map(|dataset| dataset.id.clone())
        else {
            return;
        };
        let enabled = self.store.is_dataset_enabled(&id);
        self.store.set_dataset_enabled(&id, !enabled);
        self.store_changed();
    }

    /// Skips or stops skipping the last app typed in (all-apps mode), or selects it or
    /// stops selecting it.
    pub fn toggle_last_app(&mut self) {
        let Some((id, name)) = self.last_app.clone() else {
            return;
        };
        let listed = !self.store.is_listed(&id);
        self.store.set_listed(&id, &name, listed);
        self.store_changed();
    }

    pub fn remove_dataset(&mut self, index: usize) {
        let Some(id) = self
            .store
            .datasets
            .get(index)
            .map(|dataset| dataset.id.clone())
        else {
            return;
        };
        self.store.remove_dataset(&id);
        self.store_changed();
    }

    /// The id, name, kind and text (one line per shortcut or value) of the dataset at
    /// `index`, for the editor.
    pub fn dataset_for_editing(
        &self,
        index: usize,
    ) -> Option<(String, String, DatasetKind, String)> {
        self.store.datasets.get(index).map(|dataset| {
            (
                dataset.id.clone(),
                dataset.name.clone(),
                dataset.kind(),
                typio_core::import::format_dataset_export(dataset),
            )
        })
    }

    /// Saves a dataset from the editor; see `Store::save_dataset`.
    pub fn save_dataset(
        &mut self,
        id: Option<&str>,
        name: &str,
        kind: DatasetKind,
        text: &str,
    ) -> Result<Vec<String>, String> {
        let (_, warnings) = self.store.save_dataset(id, name, kind, text)?;
        self.store_changed();
        Ok(warnings)
    }

    pub fn add_app(&mut self, id: &str, name: &str) {
        self.store.set_listed(id, name, true);
        self.store_changed();
    }

    pub fn remove_app(&mut self, index: usize) {
        self.store.remove_listed(index);
        self.store_changed();
    }

    pub fn set_app_mode(&mut self, mode: AppMode) {
        self.store.app_mode = mode;
        self.store_changed();
    }

    pub fn set_language(&mut self, language: Language) {
        self.store.language = language;
        crate::i18n::set(language);
        self.store_changed();
    }

    pub fn set_trigger(&mut self, trigger: char) {
        self.store.trigger = trigger;
        self.store_changed();
    }

    /// Turns Typio on or off in every text field (Ctrl+Alt+T or the menu).
    pub fn toggle_enabled(&mut self) {
        self.store.enabled = !self.store.enabled;
        self.store_changed();
        info!("Typio enabled: {}", self.store.enabled);
        let message = match (self.store.enabled, self.store.datasets.is_empty()) {
            (true, true) => tr(T::TurnedOnImport),
            (true, false) => tr(T::TurnedOn),
            (false, _) => tr(T::TurnedOff),
        };
        let anchor = focused_field().ok().as_ref().and_then(anchor_of);
        self.show_message(message, anchor);
    }

    fn show_message(&mut self, text: &str, anchor: Option<Rect>) {
        self.results.clear();
        self.sync_hook();
        let Some(anchor) = anchor else {
            return;
        };
        self.popup.show_message(text, anchor);
        super::set_timer(super::TIMER_MESSAGE, MESSAGE_DURATION);
    }

    /// Hides a notice once its time is up, unless suggestions replaced it.
    pub fn message_expired(&mut self) {
        if self.results.is_empty() {
            self.popup.hide();
        }
    }
}

fn utf16_slice(text: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = end.min(units.len());
    String::from_utf16_lossy(&units[start.min(end)..end])
}

fn anchor_of(field: &Field) -> Option<Rect> {
    field.info.caret_rect.or(field.info.field_rect)
}

/// Password fields and fields labelled like secrets never get suggestions.
fn is_blocked(field: &Field) -> bool {
    field.info.secure
        || is_sensitive_label(&format!("{} {}", field.info.identifier, field.info.label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use typio_platform::Modifiers;

    fn key(key: Key) -> InputEvent {
        InputEvent::KeyDown {
            key,
            modifiers: Modifiers::default(),
        }
    }

    fn state(listing: bool) -> HookState {
        HookState {
            listing,
            count: 3,
            selected: None,
            undo_armed: false,
        }
    }

    #[test]
    fn keys_move_through_the_list_and_insert() {
        let mut state = state(true);
        let mut sent = Vec::new();
        assert_eq!(
            decide_with(&mut state, key(Key::Up), &mut sent),
            Disposition::Swallow
        );
        assert_eq!(state.selected, Some(2));
        assert_eq!(
            decide_with(&mut state, key(Key::Down), &mut sent),
            Disposition::Swallow
        );
        assert_eq!(state.selected, Some(0));
        assert_eq!(
            decide_with(&mut state, key(Key::Return), &mut sent),
            Disposition::Swallow
        );
        assert!(matches!(sent.last(), Some(Command::Insert(0))));
        assert!(!state.listing);
    }

    #[test]
    fn enter_without_a_highlight_keeps_its_meaning() {
        let mut state = state(true);
        let mut sent = Vec::new();
        assert_eq!(
            decide_with(&mut state, key(Key::Return), &mut sent),
            Disposition::Pass
        );
        assert!(matches!(sent.last(), Some(Command::Hide)));
    }

    #[test]
    fn only_the_next_backspace_undoes() {
        let mut state = state(false);
        let mut sent = Vec::new();
        state.undo_armed = true;
        assert_eq!(
            decide_with(&mut state, key(Key::Backspace), &mut sent),
            Disposition::Swallow
        );
        assert!(matches!(sent.last(), Some(Command::Undo)));
        assert_eq!(
            decide_with(&mut state, key(Key::Backspace), &mut sent),
            Disposition::Pass
        );
        state.undo_armed = true;
        decide_with(&mut state, key(Key::Other), &mut sent);
        assert_eq!(
            decide_with(&mut state, key(Key::Backspace), &mut sent),
            Disposition::Pass
        );
    }

    #[test]
    fn control_alt_t_toggles() {
        let mut state = state(false);
        let mut sent = Vec::new();
        let event = InputEvent::KeyDown {
            key: Key::T,
            modifiers: Modifiers {
                control: true,
                option: true,
                ..Modifiers::default()
            },
        };
        assert_eq!(
            decide_with(&mut state, event, &mut sent),
            Disposition::Swallow
        );
        assert!(matches!(sent.last(), Some(Command::Toggle)));
    }
}
