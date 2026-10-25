//! Write single values back into the user's `config.toml`.
//!
//! The OSD bar is placed by dragging it around and the window mode is picked
//! from the tray, not by editing the file, so those choices have to survive a
//! restart. Patching the document with `toml_edit` keeps every comment
//! waylyrics regenerates on startup in place.

use std::path::PathBuf;

use toml_edit::{value, DocumentMut, Item};

use crate::log::{error, info};

fn config_path() -> Option<PathBuf> {
    if let Some(path) = crate::utils::CUSTOM_CONFIG_PATH.get() {
        return Some(path.clone());
    }

    let proj_dirs = directories::ProjectDirs::from("io", "poly000", "waylyrics")?;
    Some(proj_dirs.config_dir().join("config.toml"))
}

/// Patch top-level keys of `config.toml`, leaving everything else as is.
///
/// Failures are logged and swallowed: losing a remembered setting is not worth
/// interrupting playback for.
fn save(pairs: Vec<(&str, Item)>) {
    let Some(path) = config_path() else {
        error!("cannot locate config.toml, settings were not saved");
        return;
    };

    let Ok(config) = std::fs::read_to_string(&path) else {
        error!("cannot read {path:?}, settings were not saved");
        return;
    };

    let mut document = match config.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(e) => {
            error!("cannot parse {path:?} ({e}), settings were not saved");
            return;
        }
    };

    let keys: Vec<_> = pairs.iter().map(|(key, _)| *key).collect();
    for (key, new) in pairs {
        document[key] = new;
    }

    if let Err(e) = std::fs::write(&path, document.to_string()) {
        error!("cannot write {path:?} ({e}), settings were not saved");
        return;
    }

    info!("saved {keys:?} to {path:?}");
}

pub fn save_integers(pairs: &[(&str, i64)]) {
    save(pairs.iter().map(|(key, new)| (*key, value(*new))).collect());
}

pub fn save_bool(key: &str, new: bool) {
    save(vec![(key, value(new))]);
}

pub fn save_float(key: &str, new: f64) {
    // two decimals are plenty for a zoom factor and keep the file readable
    save(vec![(key, value((new * 100.0).round() / 100.0))]);
}

/// Remember where the user dragged the lyrics to.
pub fn save_lyric_offset(x: i32, y: i32) {
    save_integers(&[("lyric-offset-x", x as i64), ("lyric-offset-y", y as i64)]);
}
