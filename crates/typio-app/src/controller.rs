//! Ties the event tap, the focused field, the store and the popup together. Everything
//! here runs on the main thread; the tap only decides what to swallow and schedules the
//! slower Accessibility work right after.

use std::cell::RefCell;
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;
use typio_core::engine::{prefixed_word_at, Suggester, WordRange};
use typio_core::sensitive::is_sensitive_label;
use typio_core::{DatasetKind, RankedSuggestion};
use typio_platform::{
    focused_field, frontmost_app, Disposition, EventTap, Field, InputEvent, Key, Rect,
};

use crate::popup::Popup;
use crate::store::{BlockedApp, Store};

/// Lets an input method finish rewriting the text before Typio reads it.
const REFRESH_DELAY: Duration = Duration::from_millis(40);
const MESSAGE_DURATION: Duration = Duration::from_millis(1600);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// What the menu bar icon, the menu and the Settings window show.
pub struct UiState {
    /// Accessibility access is granted and Typio is listening.
    pub trusted: bool,
    pub enabled: bool,
    pub trigger: char,
    pub datasets: Vec<DatasetRow>,
    pub blocked_apps: Vec<BlockedApp>,
}

pub struct DatasetRow {
    pub name: String,
    /// For example "99 shortcuts".
    pub detail: String,
    pub enabled: bool,
}

/// Brings the menu bar icon and the Settings window up to date, right after the current
/// event finishes.
pub fn refresh_ui() {
    let _ = DispatchQueue::main().after(DispatchTime::NOW, || {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(state) = with_app(|app| app.ui_state()) else {
            return;
        };
        crate::menu::update_icon(mtm, &state);
        crate::settings::update(mtm, &state);
    });
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
    /// Indexed datasets, rebuilt when the store changes.
    suggester: Option<(u64, Suggester)>,
    store_version: u64,
    refresh_generation: u64,
    message_generation: u64,
    /// The last Accessibility state written to the log, so polling logs only changes.
    logged_trust: Option<bool>,
}

pub fn install(store: Store, mtm: MainThreadMarker) {
    let app = App {
        store,
        popup: Popup::new(mtm),
        tap: None,
        field: None,
        word: None,
        results: Vec::new(),
        dismissed: None,
        suggester: None,
        store_version: 0,
        refresh_generation: 0,
        message_generation: 0,
        logged_trust: None,
    };
    APP.with(|cell| *cell.borrow_mut() = Some(app));
}

/// Runs `action` on the app; `None` while the app is already busy (for example inside a
/// modal dialog started from a menu action).
pub fn with_app<R>(action: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(action))
}

fn later(delay: Duration, action: impl FnOnce(&mut App) + Send + 'static) {
    let when = DispatchTime::try_from(delay).unwrap_or(DispatchTime::NOW);
    let _ = DispatchQueue::main().after(when, move || {
        with_app(action);
    });
}

impl App {
    pub fn has_tap(&self) -> bool {
        self.tap.is_some()
    }

    /// Installs the event tap once Accessibility access is granted, checking again every
    /// two seconds until then.
    pub fn start_tap(&mut self) {
        if self.tap.is_some() {
            return;
        }
        let trusted = typio_platform::is_trusted(false);
        if self.logged_trust != Some(trusted) {
            info!("Accessibility access: {trusted}");
            // macOS keeps refusing Accessibility calls to other apps from a process that
            // started without access, even after it is granted; only a new process works.
            if trusted && self.logged_trust == Some(false) {
                relaunch();
                return;
            }
            self.logged_trust = Some(trusted);
        }
        if trusted {
            let tap = EventTap::install(|event| {
                // Only special keys are named; typed characters all log as `Other`.
                if let InputEvent::KeyDown { key, modifiers } = event {
                    debug!("key {key:?} {modifiers:?}");
                }
                let disposition = with_app(|app| app.handle(event));
                if disposition.is_none() {
                    debug!("event while busy, passed through");
                }
                disposition.unwrap_or(Disposition::Pass)
            });
            match tap {
                Ok(tap) => {
                    info!("Event tap installed");
                    self.tap = Some(tap);
                    refresh_ui();
                }
                Err(error) => info!("Event tap failed: {error}"),
            }
        }
        if self.tap.is_none() {
            later(Duration::from_secs(2), App::start_tap);
        }
    }

    pub fn store_changed(&mut self) {
        self.store_version += 1;
        refresh_ui();
        if let Err(error) = self.store.save() {
            info!("Could not save data: {error}");
        }
    }

    /// Decides synchronously whether the tap swallows `event`.
    fn handle(&mut self, event: InputEvent) -> Disposition {
        match event {
            InputEvent::KeyDown {
                key: Key::T,
                modifiers,
            } if modifiers.control
                && modifiers.option
                && !modifiers.command
                && !modifiers.shift =>
            {
                later(Duration::ZERO, App::toggle_enabled);
                Disposition::Swallow
            }
            InputEvent::KeyDown { key, modifiers } if self.popup.is_visible() => {
                let plain = !modifiers.command && !modifiers.control && !modifiers.option;
                match key {
                    Key::Down | Key::Up if plain => {
                        let count = self.results.len();
                        let next = match (self.popup.selected(), key) {
                            (None, Key::Down) => 0,
                            (None, _) => count - 1,
                            (Some(index), Key::Down) => (index + 1) % count,
                            (Some(index), _) => (index + count - 1) % count,
                        };
                        self.popup.select(Some(next));
                        Disposition::Swallow
                    }
                    // Enter and Tab only insert an item the user highlighted; otherwise they
                    // keep their usual meaning.
                    Key::Return | Key::Tab if plain => match self.popup.selected() {
                        Some(index) => {
                            later(Duration::ZERO, move |app| app.insert(index));
                            Disposition::Swallow
                        }
                        None => {
                            self.popup.hide();
                            Disposition::Pass
                        }
                    },
                    Key::Escape => {
                        self.dismissed = self.field.as_ref().map(|field| field.info.value.clone());
                        self.popup.hide();
                        Disposition::Swallow
                    }
                    _ => {
                        if !plain {
                            self.popup.hide();
                        }
                        self.schedule_refresh();
                        Disposition::Pass
                    }
                }
            }
            InputEvent::KeyDown { .. } => {
                self.schedule_refresh();
                Disposition::Pass
            }
            InputEvent::MouseDown { x, y } => match self.popup.row_at(x, y) {
                Some(index) => {
                    later(Duration::ZERO, move |app| app.insert(index));
                    Disposition::Swallow
                }
                None => {
                    self.popup.hide();
                    Disposition::Pass
                }
            },
        }
    }

    fn schedule_refresh(&mut self) {
        self.refresh_generation += 1;
        let generation = self.refresh_generation;
        later(REFRESH_DELAY, move |app| {
            if app.refresh_generation == generation {
                app.refresh();
            }
        });
    }

    fn refresh(&mut self) {
        self.results.clear();
        self.field = None;
        let field = match focused_field() {
            Ok(field) => field,
            Err(error) => {
                debug!("refresh: {error:?}");
                self.popup.hide();
                return;
            }
        };
        let info = &field.info;
        if !self.store.enabled || is_blocked(&field) || self.store.is_app_blocked(&info.app_id) {
            debug!("refresh: {} {} is off or blocked", info.app_id, info.role);
            self.popup.hide();
            return;
        };
        if self.dismissed.as_deref() == Some(info.value.as_str()) {
            self.popup.hide();
            self.field = Some(field);
            return;
        }
        self.dismissed = None;

        if !matches!(&self.suggester, Some((version, _)) if *version == self.store_version) {
            let suggester = Suggester::new(self.store.enabled_datasets());
            self.suggester = Some((self.store_version, suggester));
        }
        let Some((_, suggester)) = &self.suggester else {
            return;
        };

        // Suggestions only come up for a word typed after a colon, such as `:xtt`.
        self.word = info
            .caret
            .and_then(|caret| prefixed_word_at(&info.value, caret, self.store.trigger));
        self.results = match &self.word {
            Some(word) => suggester.suggest(&word.text, None),
            None => Vec::new(),
        };
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
            _ => self.popup.hide(),
        }
        self.field = Some(field);
    }

    fn insert(&mut self, index: usize) {
        self.popup.hide();
        let (Some(field), Some(choice), Some(word)) = (
            self.field.take(),
            self.results.get(index).cloned(),
            self.word.as_ref(),
        ) else {
            return;
        };
        // The colon and the word after it become the chosen text.
        if field
            .replace(word.start, word.end - word.start, &choice.value)
            .is_err()
        {
            self.show_message("Typio can’t type into this field", anchor_of(&field));
        }
        self.results.clear();
    }

    pub fn ui_state(&self) -> UiState {
        let datasets = self
            .store
            .datasets
            .iter()
            .map(|dataset| DatasetRow {
                name: dataset.name.clone(),
                detail: match dataset.kind() {
                    DatasetKind::Shortcuts => format!(
                        "{} shortcuts",
                        dataset.shortcuts.as_ref().map_or(0, Vec::len)
                    ),
                    DatasetKind::Values => format!("{} values", dataset.values.len()),
                },
                enabled: self.store.is_dataset_enabled(&dataset.id),
            })
            .collect();
        UiState {
            trusted: self.has_tap(),
            enabled: self.store.enabled,
            trigger: self.store.trigger,
            datasets,
            blocked_apps: self.store.blocked_apps.clone(),
        }
    }

    /// `index` is the dataset's position in the store, as in `UiState::datasets`.
    pub fn set_dataset_enabled(&mut self, index: usize, enabled: bool) {
        let Some(id) = self
            .store
            .datasets
            .get(index)
            .map(|dataset| dataset.id.clone())
        else {
            return;
        };
        self.store.set_dataset_enabled(&id, enabled);
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

    /// Blocks or unblocks the app in front (the menu), with a notice at the caret.
    pub fn toggle_frontmost_app(&mut self) {
        let Some((id, name)) = frontmost_app() else {
            return;
        };
        let blocked = !self.store.is_app_blocked(&id);
        self.store.set_app_blocked(&id, &name, blocked);
        self.store_changed();
        let message = if blocked {
            format!("Typio off in {name}")
        } else {
            format!("Typio on in {name}")
        };
        let anchor = focused_field().ok().as_ref().and_then(anchor_of);
        self.show_message(&message, anchor);
    }

    pub fn unblock_app(&mut self, index: usize) {
        if index < self.store.blocked_apps.len() {
            self.store.blocked_apps.remove(index);
            self.store_changed();
        }
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

    pub fn set_trigger(&mut self, trigger: char) {
        self.store.trigger = trigger;
        self.store_changed();
    }

    /// Turns Typio on or off in every text field (⌃⌥T or the menu).
    pub fn toggle_enabled(&mut self) {
        self.store.enabled = !self.store.enabled;
        self.store_changed();
        info!("Typio enabled: {}", self.store.enabled);
        let message = match (self.store.enabled, self.store.datasets.is_empty()) {
            (true, true) => "Typio on. Import a dataset from the menu bar.",
            (true, false) => "Typio on",
            (false, _) => "Typio off",
        };
        let anchor = focused_field().ok().as_ref().and_then(anchor_of);
        self.show_message(message, anchor);
    }

    fn show_message(&mut self, text: &str, anchor: Option<Rect>) {
        let Some(anchor) = anchor else {
            return;
        };
        self.results.clear();
        self.popup.show_message(text, anchor);
        self.message_generation += 1;
        let generation = self.message_generation;
        later(MESSAGE_DURATION, move |app| {
            if app.message_generation == generation && app.results.is_empty() {
                app.popup.hide();
            }
        });
    }
}

fn anchor_of(field: &Field) -> Option<Rect> {
    field.info.caret_rect.or(field.info.field_rect)
}

/// Password fields and fields labelled like secrets never get suggestions.
fn is_blocked(field: &Field) -> bool {
    field.info.secure
        || is_sensitive_label(&format!("{} {}", field.info.identifier, field.info.label))
}

/// Starts a fresh copy of the app bundle and quits this one.
fn relaunch() {
    let executable = std::env::current_exe().unwrap_or_default();
    // …/Typio.app/Contents/MacOS/Typio
    let Some(bundle) = executable
        .ancestors()
        .nth(3)
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
    else {
        info!("Not running from an app bundle; quit and reopen Typio to finish enabling it");
        return;
    };
    let mut command = std::process::Command::new("/usr/bin/open");
    command.arg("-n").arg(bundle);
    // Keep development settings for the new process.
    for name in ["TYPIO_DATA_DIR", "TYPIO_DEBUG"] {
        if let Some(value) = std::env::var_os(name) {
            command
                .arg("--env")
                .arg(format!("{name}={}", value.to_string_lossy()));
        }
    }
    match command.spawn() {
        Ok(_) => {
            info!("Relaunching to finish enabling Accessibility access");
            std::process::exit(0);
        }
        Err(error) => info!("Could not relaunch: {error}; quit and reopen Typio"),
    }
}
