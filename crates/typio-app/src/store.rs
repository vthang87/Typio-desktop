//! Datasets and the on/off switch, saved as one JSON file in the user's data folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use typio_core::backup::{format_backup, parse_backup, MAX_BACKUP_BYTES};
use typio_core::import::{
    looks_like_shortcuts, parse_shortcuts, parse_suggestion_file, parse_text_suggestions,
};
use typio_core::{DatasetKind, SuggestionDataset};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub schema_version: u32,
    pub datasets: Vec<SuggestionDataset>,
    /// Suggest in every text field. Files from older versions have per-field rules
    /// instead; those are ignored and dropped on the next save.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// Ids of datasets that are kept but do not supply suggestions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_datasets: Vec<String>,
    /// Whether Typio runs in every app except `blocked_apps`, or only in `selected_apps`.
    #[serde(default)]
    pub app_mode: AppMode,
    /// Apps skipped in `AppMode::All`. Browsers start here because the Typio extension
    /// handles them.
    #[serde(default = "default_blocked_apps")]
    pub blocked_apps: Vec<AppRef>,
    /// The only apps Typio runs in under `AppMode::Selected`.
    #[serde(default)]
    pub selected_apps: Vec<AppRef>,
    /// Typed before a word to bring up suggestions, as in `:xtt`.
    #[serde(default = "default_trigger")]
    pub trigger: char,
    #[serde(skip)]
    path: PathBuf,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            datasets: Vec::new(),
            enabled: true,
            disabled_datasets: Vec::new(),
            app_mode: AppMode::All,
            blocked_apps: default_blocked_apps(),
            selected_apps: Vec::new(),
            trigger: default_trigger(),
            path: PathBuf::new(),
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    /// Every app except the skipped ones.
    #[default]
    All,
    /// Only the selected apps.
    Selected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRef {
    /// Bundle identifier.
    pub id: String,
    pub name: String,
}

fn default_blocked_apps() -> Vec<AppRef> {
    [
        ("com.google.Chrome", "Google Chrome"),
        ("com.microsoft.edgemac", "Microsoft Edge"),
        ("com.brave.Browser", "Brave Browser"),
        ("company.thebrowser.Browser", "Arc"),
    ]
    .into_iter()
    .map(|(id, name)| AppRef {
        id: id.into(),
        name: name.into(),
    })
    .collect()
}

fn default_trigger() -> char {
    ':'
}

/// Characters offered as the trigger in Settings.
pub const TRIGGERS: [char; 8] = [':', ';', '/', '\\', '#', '@', '!', '~'];

impl Store {
    /// `TYPIO_DATA_DIR` overrides the folder, for trying Typio without touching real data.
    pub fn data_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("TYPIO_DATA_DIR") {
            return PathBuf::from(dir);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        home.join("Library/Application Support/Typio")
    }

    /// Loads the store, or starts an empty one when the file does not exist yet.
    pub fn load(dir: &Path) -> io::Result<Self> {
        let path = dir.join("data.json");
        let mut store = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Store>(&text).map_err(io::Error::other)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Store::default(),
            Err(error) => return Err(error),
        };
        store.schema_version = SCHEMA_VERSION;
        store.path = path;
        Ok(store)
    }

    /// Writes to a temporary file first so a crash never leaves half a file.
    pub fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )?;
        fs::rename(temporary, &self.path)
    }

    /// Whether Typio runs in the app with bundle identifier `id`.
    pub fn runs_in(&self, id: &str) -> bool {
        match self.app_mode {
            AppMode::All => !self.is_listed(id),
            AppMode::Selected => self.is_listed(id),
        }
    }

    /// The list the current mode uses: skipped apps or selected apps.
    pub fn app_list(&self) -> &[AppRef] {
        match self.app_mode {
            AppMode::All => &self.blocked_apps,
            AppMode::Selected => &self.selected_apps,
        }
    }

    fn app_list_mut(&mut self) -> &mut Vec<AppRef> {
        match self.app_mode {
            AppMode::All => &mut self.blocked_apps,
            AppMode::Selected => &mut self.selected_apps,
        }
    }

    pub fn is_listed(&self, id: &str) -> bool {
        self.app_list().iter().any(|app| app.id == id)
    }

    /// Adds the app to, or removes it from, the current mode's list.
    pub fn set_listed(&mut self, id: &str, name: &str, listed: bool) {
        let list = self.app_list_mut();
        list.retain(|app| app.id != id);
        if listed {
            list.push(AppRef {
                id: id.into(),
                name: name.into(),
            });
        }
    }

    pub fn remove_listed(&mut self, index: usize) {
        let list = self.app_list_mut();
        if index < list.len() {
            list.remove(index);
        }
    }

    pub fn is_dataset_enabled(&self, id: &str) -> bool {
        !self.disabled_datasets.iter().any(|disabled| disabled == id)
    }

    pub fn set_dataset_enabled(&mut self, id: &str, enabled: bool) {
        self.disabled_datasets.retain(|disabled| disabled != id);
        if !enabled {
            self.disabled_datasets.push(id.to_string());
        }
    }

    /// Datasets that supply suggestions.
    pub fn enabled_datasets(&self) -> impl Iterator<Item = &SuggestionDataset> {
        self.datasets
            .iter()
            .filter(|dataset| self.is_dataset_enabled(&dataset.id))
    }

    pub fn remove_dataset(&mut self, id: &str) {
        self.datasets.retain(|dataset| dataset.id != id);
        self.disabled_datasets.retain(|disabled| disabled != id);
    }

    /// Saves a dataset edited as text: `trigger: expansion` lines for shortcuts, one value
    /// per line otherwise. `id` is `None` for a new dataset, which gets `kind`; an existing
    /// one keeps its kind. Returns the id and warnings, such as triggers that other
    /// datasets also use, or why nothing was saved.
    pub fn save_dataset(
        &mut self,
        id: Option<&str>,
        name: &str,
        kind: DatasetKind,
        text: &str,
    ) -> Result<(String, Vec<String>), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the dataset a name.".into());
        }
        let existing = match id {
            Some(id) => Some(
                self.datasets
                    .iter()
                    .position(|dataset| dataset.id == id)
                    .ok_or("This dataset no longer exists.")?,
            ),
            None => None,
        };
        let kind = existing.map_or(kind, |index| self.datasets[index].kind());

        let mut warnings = Vec::new();
        let (values, shortcuts) = match kind {
            DatasetKind::Shortcuts => {
                let parsed = parse_shortcuts(text);
                if !parsed.invalid_lines.is_empty() {
                    let lines: Vec<String> =
                        parsed.invalid_lines.iter().map(usize::to_string).collect();
                    return Err(format!("Line {} is not “trigger: text”.", lines.join(", ")));
                }
                if let Some(trigger) = parsed.duplicates.first() {
                    return Err(format!("The trigger “{trigger}” is used more than once."));
                }
                for shortcut in &parsed.shortcuts {
                    let trigger = shortcut.trigger.to_lowercase();
                    let others = self.datasets.iter().enumerate().filter(|(index, dataset)| {
                        Some(*index) != existing
                            && dataset
                                .shortcuts
                                .iter()
                                .flatten()
                                .any(|other| other.trigger.to_lowercase() == trigger)
                    });
                    for (_, other) in others {
                        warnings.push(format!(
                            "“{}” is also a trigger in “{}”.",
                            shortcut.trigger, other.name
                        ));
                    }
                }
                (Vec::new(), Some(parsed.shortcuts))
            }
            DatasetKind::Values => (parse_text_suggestions(text), None),
        };
        if values.is_empty() && shortcuts.as_ref().is_none_or(Vec::is_empty) {
            return Err("Add at least one line.".into());
        }

        let dataset = match existing {
            Some(index) => &mut self.datasets[index],
            None => {
                self.datasets.push(SuggestionDataset {
                    id: new_id(),
                    name: String::new(),
                    kind: Some(kind),
                    values: Vec::new(),
                    shortcuts: None,
                    created_at: now(),
                    updated_at: String::new(),
                });
                self.datasets.last_mut().expect("just pushed")
            }
        };
        dataset.name = name.to_string();
        dataset.values = values;
        dataset.shortcuts = shortcuts;
        dataset.updated_at = now();
        Ok((dataset.id.clone(), warnings))
    }

    /// Every dataset as a backup file the extension and this app can import.
    pub fn backup(&self) -> String {
        format_backup(&self.datasets, &now())
    }

    /// Adds the datasets in a TXT, CSV or JSON file, or in a Typio extension backup.
    /// Returns a message for the user.
    pub fn import(&mut self, path: &Path) -> Result<String, String> {
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_BACKUP_BYTES {
            return Err("This file is larger than 10 MB.".into());
        }
        let text =
            String::from_utf8(bytes).map_err(|_| "This file is not UTF-8 text.".to_string())?;
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let is_json = filename.to_lowercase().ends_with(".json");

        if is_json && text.contains("\"app\"") {
            if let Ok(backup) = parse_backup(&text) {
                let count = backup.suggestion_datasets.len();
                for dataset in backup.suggestion_datasets {
                    match self
                        .datasets
                        .iter_mut()
                        .find(|existing| existing.id == dataset.id)
                    {
                        Some(existing) => *existing = dataset,
                        None => self.datasets.push(dataset),
                    }
                }
                return Ok(format!("Imported {count} datasets from the Typio backup."));
            }
        }

        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Dataset")
            .to_string();
        let mut dataset = SuggestionDataset {
            id: new_id(),
            name,
            kind: None,
            values: Vec::new(),
            shortcuts: None,
            created_at: now(),
            updated_at: now(),
        };
        let message = if !is_json && looks_like_shortcuts(&text) {
            let parsed = parse_shortcuts(&text);
            let count = parsed.shortcuts.len();
            dataset.kind = Some(DatasetKind::Shortcuts);
            dataset.shortcuts = Some(parsed.shortcuts);
            let mut message = format!("Imported {count} shortcuts into “{}”.", dataset.name);
            if !parsed.duplicates.is_empty() {
                message += &format!(" Skipped {} duplicate triggers.", parsed.duplicates.len());
            }
            message
        } else {
            dataset.values = parse_suggestion_file(filename, &text)
                .map_err(|_| "This file is not valid JSON.".to_string())?;
            format!(
                "Imported {} values into “{}”.",
                dataset.values.len(),
                dataset.name
            )
        };
        if dataset.values.is_empty() && dataset.shortcuts.as_ref().is_none_or(Vec::is_empty) {
            return Err("This file has no values to import.".into());
        }
        self.datasets.push(dataset);
        Ok(message)
    }
}

fn now() -> String {
    humantime::format_rfc3339_millis(SystemTime::now()).to_string()
}

fn new_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:x}{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("typio-test-{name}-{}", new_id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_data_saved_with_field_rules() {
        let dir = temp_dir("legacy");
        fs::write(
            dir.join("data.json"),
            r#"{"schemaVersion":1,"datasets":[],"rules":[{"id":"r"}],"allFields":false,"excluded":[]}"#,
        )
        .unwrap();
        let store = Store::load(&dir).unwrap();
        assert!(store.enabled, "older files load turned on");
        assert_eq!(store.trigger, ':');
        assert!(
            !store.runs_in("com.google.Chrome"),
            "browsers start skipped"
        );
        assert!(store.runs_in("com.apple.Notes"));
        store.save().unwrap();
        let saved = fs::read_to_string(dir.join("data.json")).unwrap();
        assert!(!saved.contains("rules"), "old rules are dropped");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn enables_and_removes_datasets() {
        let dir = temp_dir("enable");
        let file = dir.join("phuong.txt");
        fs::write(&file, "xtt:Xã Tam Tiến\npbg:Phường Bắc Giang\n").unwrap();
        let mut store = Store::load(&dir).unwrap();
        store.import(&file).unwrap();
        store.import(&file).unwrap();
        let first = store.datasets[0].id.clone();
        assert_eq!(store.enabled_datasets().count(), 2);

        store.set_dataset_enabled(&first, false);
        store.set_dataset_enabled(&first, false);
        assert_eq!(store.disabled_datasets, vec![first.clone()]);
        assert_eq!(store.enabled_datasets().count(), 1);
        store.trigger = '/';
        store.set_listed("com.google.Chrome", "Google Chrome", false);
        store.set_listed("com.apple.Notes", "Notes", true);
        store.set_listed("com.apple.Notes", "Notes", true);
        assert_eq!(store.blocked_apps.len(), 4);
        store.app_mode = AppMode::Selected;
        assert!(!store.runs_in("com.apple.TextEdit"), "nothing selected yet");
        store.set_listed("com.apple.TextEdit", "TextEdit", true);
        assert!(store.runs_in("com.apple.TextEdit"));
        assert!(
            !store.runs_in("com.apple.Notes"),
            "the skip list does not apply"
        );
        store.remove_listed(5);
        store.remove_listed(0);
        assert!(store.selected_apps.is_empty());
        store.set_listed("com.apple.TextEdit", "TextEdit", true);
        store.save().unwrap();

        let mut loaded = Store::load(&dir).unwrap();
        assert!(!loaded.is_dataset_enabled(&first));
        assert_eq!(loaded.app_mode, AppMode::Selected);
        assert!(loaded.runs_in("com.apple.TextEdit"));
        loaded.app_mode = AppMode::All;
        assert!(!loaded.runs_in("com.apple.Notes"));
        assert!(loaded.runs_in("com.google.Chrome"));
        assert_eq!(loaded.trigger, '/');
        loaded.remove_dataset(&first);
        assert_eq!(loaded.datasets.len(), 1);
        assert!(loaded.disabled_datasets.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saves_edited_datasets() {
        let mut store = Store::default();
        let (id, warnings) = store
            .save_dataset(
                None,
                " Phường ",
                DatasetKind::Shortcuts,
                "xtt: Xã Tam Tiến\nTT:Tam Tiến\n",
            )
            .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(store.datasets[0].name, "Phường");
        assert_eq!(store.datasets[0].shortcuts.as_ref().unwrap().len(), 2);

        let (_, warnings) = store
            .save_dataset(None, "Khác", DatasetKind::Shortcuts, "XTT:Xóm Tân Tiến")
            .unwrap();
        assert_eq!(warnings, vec!["“XTT” is also a trigger in “Phường”."]);

        assert_eq!(
            store.save_dataset(Some(&id), "Phường", DatasetKind::Values, "xtt:A\nxtt:B"),
            Err("The trigger “xtt” is used more than once.".into()),
            "an existing dataset keeps its kind"
        );
        assert_eq!(
            store.save_dataset(Some(&id), "Phường", DatasetKind::Shortcuts, "xtt A"),
            Err("Line 1 is not “trigger: text”.".into())
        );
        assert!(store
            .save_dataset(Some(&id), "", DatasetKind::Shortcuts, "a:b")
            .is_err());
        assert!(store
            .save_dataset(None, "Tên", DatasetKind::Values, "\n")
            .is_err());

        store
            .save_dataset(
                Some(&id),
                "Phường mới",
                DatasetKind::Shortcuts,
                "pbg:Phường Bắc Giang",
            )
            .unwrap();
        assert_eq!(store.datasets.len(), 2);
        assert_eq!(store.datasets[0].name, "Phường mới");
        store
            .save_dataset(None, "Tên", DatasetKind::Values, "An\nBình\nAn")
            .unwrap();
        assert_eq!(store.datasets[2].values, vec!["An", "Bình"]);
    }

    #[test]
    fn backups_round_trip_through_import() {
        let dir = temp_dir("backup-export");
        let mut store = Store::default();
        store
            .save_dataset(None, "Phường", DatasetKind::Shortcuts, "xtt:Xã Tam Tiến")
            .unwrap();
        let file = dir.join("typio-datasets.json");
        fs::write(&file, store.backup()).unwrap();

        let mut other = Store::load(&dir).unwrap();
        assert_eq!(
            other.import(&file).unwrap(),
            "Imported 1 datasets from the Typio backup."
        );
        assert_eq!(other.datasets, store.datasets);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imports_files_and_round_trips() {
        let dir = temp_dir("import");
        let shortcuts = dir.join("phuong.txt");
        fs::write(
            &shortcuts,
            "\u{feff}pbg:Phường Bắc Giang\nxtt:Xã Tam Tiến\npbg:Trùng\n",
        )
        .unwrap();
        let values = dir.join("ten.csv");
        fs::write(&values, "Nguyễn Văn An,Lê Thị Bình").unwrap();
        let empty = dir.join("empty.txt");
        fs::write(&empty, "\n\n").unwrap();

        let mut store = Store::load(&dir).unwrap();
        assert_eq!(
            store.import(&shortcuts).unwrap(),
            "Imported 2 shortcuts into “phuong”. Skipped 1 duplicate triggers."
        );
        assert_eq!(
            store.import(&values).unwrap(),
            "Imported 2 values into “ten”."
        );
        assert!(store.import(&empty).is_err());
        store.enabled = false;
        store.save().unwrap();

        let loaded = Store::load(&dir).unwrap();
        assert_eq!(loaded.datasets, store.datasets);
        assert!(!loaded.enabled);
        assert_eq!(loaded.datasets[0].kind(), DatasetKind::Shortcuts);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imports_extension_backups() {
        let dir = temp_dir("backup");
        let backup = dir.join("typio-datasets-2026-11-06.json");
        fs::write(
            &backup,
            r#"{"app":"typio","schemaVersion":1,"scope":"datasets","exportedAt":"",
                "suggestionDatasets":[{"id":"d1","name":"Tên","values":["An"],"createdAt":"","updatedAt":""}]}"#,
        )
        .unwrap();
        let mut store = Store::load(&dir).unwrap();
        assert_eq!(
            store.import(&backup).unwrap(),
            "Imported 1 datasets from the Typio backup."
        );
        assert_eq!(
            store.import(&backup).unwrap(),
            "Imported 1 datasets from the Typio backup."
        );
        assert_eq!(
            store.datasets.len(),
            1,
            "re-importing replaces datasets with the same id"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
