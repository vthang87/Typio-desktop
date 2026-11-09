//! A small log at `~/Library/Logs/Typio.log`, since a menu bar app has no console.
//! Callers never pass typed text or field values.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;

use crate::store::Store;

fn path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Library/Logs/Typio.log")
}

/// Detailed logging: `TYPIO_DEBUG` is set, or a file named `debug` is in the data folder.
pub fn verbose() -> bool {
    static VERBOSE: OnceLock<bool> = OnceLock::new();
    *VERBOSE.get_or_init(|| {
        std::env::var_os("TYPIO_DEBUG").is_some() || Store::data_dir().join("debug").exists()
    })
}

pub fn write(message: std::fmt::Arguments) {
    let line = format!(
        "{} {message}\n",
        humantime::format_rfc3339_millis(SystemTime::now())
    );
    eprint!("{line}");
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}
