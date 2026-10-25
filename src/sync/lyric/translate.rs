//! Automatic translation of the lyric that was just loaded.
//!
//! Providers almost never ship an Italian translation (NetEase's Chinese one is
//! discarded on load), so whenever a timed lyric arrives without one, it is
//! translated in two passes sharing the original timestamps:
//!
//! 1. quick pass: the configured backend (NLLB-200 by default) in chunks of a
//!    few lines, each shown as soon as it is back, so the first verses are
//!    translated within a second;
//! 2. refinement: an LLM CLI (`refine_backend`) rewrites the whole song in
//!    natural Italian, getting slang, swearing and idioms right, and replaces
//!    the quick translation when it is done.
//!
//! The result is cached together with a "refined" flag, so each song goes
//! through the slow pass once.

use std::cell::Cell;
use std::sync::{LazyLock, OnceLock};

use crate::config::TranslationConfig;
use crate::log::*;
use crate::lyric_providers::{LyricLineOwned, LyricOwned};
use crate::sync::lyric::cache::refresh_lyric_cache;
use crate::sync::{LyricState, TrackState, LYRIC, TRACK_PLAYING_STATE};

/// set once at startup from the `[translation]` table of `config.toml`
pub static TRANSLATION_CONFIG: OnceLock<TranslationConfig> = OnceLock::new();

/// lines per request: small enough for the first verses to show up quickly
const CHUNK_SIZE: usize = 8;

thread_local! {
    /// whether the current translation already went through the refinement
    static REFINED: Cell<bool> = const { Cell::new(false) };
}

pub fn set_refined(refined: bool) {
    REFINED.set(refined);
}

pub fn is_refined() -> bool {
    REFINED.get()
}

/// Translate the current lyric: the quick pass if it has no translation yet,
/// then the refinement if it has not been refined yet.
///
/// `cache` tells whether the results should be written back to the lyric
/// cache, so the song is not translated again next time.
pub async fn translate_missing(cache: bool) {
    let Some(config) = TRANSLATION_CONFIG.get() else {
        return;
    };

    let Some((origin, has_translation)) =
        LYRIC.with_borrow(|LyricState { origin, translation }| match origin {
            LyricOwned::LineTimestamp(lines) => Some((lines.clone(), translation.has_lines())),
            _ => None,
        })
    else {
        return;
    };

    // choruses repeat a lot: translate every distinct line once
    let mut unique: Vec<&str> = vec![];
    for line in &origin {
        let text = line.text.trim();
        if !text.is_empty() && !unique.contains(&text) {
            unique.push(text);
        }
    }
    if unique.is_empty() {
        return;
    }

    if !has_translation {
        if !quick_pass(config, &origin, &unique).await {
            return;
        }
        save_cache(cache);
    }

    let refine = config.refine_backend != "none"
        && crate::translation::is_italian(&config.target_lang)
        && !is_refined();
    if refine && refine_pass(config, &origin, &unique).await {
        save_cache(cache);
    }
}

/// Map translated distinct lines back onto the song's timestamps, and apply
/// them if the song is still the one playing.
///
/// Lines with no translation in `translated` keep their current one.
fn apply(origin: &[LyricLineOwned], unique: &[&str], translated: &[Option<String>]) -> bool {
    LYRIC.with_borrow_mut(|state| {
        let LyricOwned::LineTimestamp(current) = &state.origin else {
            return false;
        };
        // the track may have changed while we were waiting for the backend
        if current != origin {
            return false;
        }

        let previous = match &state.translation {
            LyricOwned::LineTimestamp(lines) => lines.clone(),
            _ => vec![],
        };
        let translation = origin
            .iter()
            .enumerate()
            .map(|(idx, line)| LyricLineOwned {
                text: unique
                    .iter()
                    .position(|text| *text == line.text.trim())
                    .and_then(|unique_idx| translated[unique_idx].clone())
                    .or_else(|| previous.get(idx).map(|line| line.text.clone()))
                    .unwrap_or_default(),
                start_time: line.start_time,
            })
            .collect();

        state.translation = LyricOwned::LineTimestamp(translation);
        true
    })
}

async fn quick_pass(config: &TranslationConfig, origin: &[LyricLineOwned], unique: &[&str]) -> bool {
    info!(
        "translating {} distinct lines with backend {}",
        unique.len(),
        config.backend
    );

    let mut translated: Vec<Option<String>> = vec![None; unique.len()];
    for (chunk_idx, chunk) in unique.chunks(CHUNK_SIZE).enumerate() {
        let texts: Vec<String> = chunk.iter().map(|text| text.to_string()).collect();
        let config = config.clone();
        let result = crate::tokio_spawn!(async move {
            crate::translation::translate_batch(&texts, &config).await
        })
        .await;

        let lines = match result {
            Ok(lines) if lines.len() == chunk.len() => lines,
            Ok(lines) => {
                warn!(
                    "translation returned {} lines for {}, giving up on this song",
                    lines.len(),
                    chunk.len()
                );
                return false;
            }
            Err(e) => {
                error!("translation task failed: {e}");
                return false;
            }
        };

        let start = chunk_idx * CHUNK_SIZE;
        for (offset, line) in lines.into_iter().enumerate() {
            translated[start + offset] = Some(line);
        }

        if !apply(origin, unique, &translated) {
            debug!("track changed, dropping the rest of the translation");
            return false;
        }
    }

    set_refined(false);
    info!("translated {} distinct lines", unique.len());
    true
}

async fn refine_pass(config: &TranslationConfig, origin: &[LyricLineOwned], unique: &[&str]) -> bool {
    // one LLM at a time: skipping through songs must not pile up processes
    static REFINE_LOCK: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::new(()));
    let _guard = REFINE_LOCK.lock().await;

    let still_playing = LYRIC.with_borrow(|state| {
        matches!(&state.origin, LyricOwned::LineTimestamp(current) if current == origin)
    });
    if !still_playing || is_refined() {
        return false;
    }

    let (title, artist) = TRACK_PLAYING_STATE.with_borrow(|TrackState { metainfo, .. }| {
        metainfo
            .as_ref()
            .map(|meta| {
                (
                    meta.title.clone().unwrap_or_default(),
                    meta.artists.as_deref().unwrap_or_default().join(", "),
                )
            })
            .unwrap_or_default()
    });

    info!(
        "refining {} lines of {title} with {}",
        unique.len(),
        config.refine_backend
    );

    let lines: Vec<String> = unique.iter().map(|text| text.to_string()).collect();
    let config = config.clone();
    let result = crate::tokio_spawn!(async move {
        crate::translation::refine_song(&lines, &title, &artist, &config).await
    })
    .await;

    let refined = match result {
        Ok(Ok(refined)) => refined,
        Ok(Err(e)) => {
            warn!("refinement skipped: {e}");
            return false;
        }
        Err(e) => {
            error!("refinement task failed: {e}");
            return false;
        }
    };

    if !apply(origin, unique, &refined) {
        debug!("track changed while refining, dropping the refinement");
        return false;
    }
    set_refined(true);
    true
}

fn save_cache(cache: bool) {
    if !cache {
        return;
    }
    if let Some(cache_path) = TRACK_PLAYING_STATE.with_borrow(|state| state.cache_path.clone()) {
        refresh_lyric_cache(&cache_path);
    }
}
