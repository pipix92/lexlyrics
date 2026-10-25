//! Automatic translation of the lyric that was just loaded.
//!
//! Providers almost never ship an Italian translation (NetEase's Chinese one is
//! discarded on load), so whenever a timed lyric arrives without one, its lines
//! go through the configured backend (NLLB-200 by default) and come back as a
//! translated lyric sharing the original timestamps.
//!
//! Lines are sent in small chunks and every chunk is shown as soon as it is
//! back, so the first verses are translated well before the whole song is.

use std::sync::OnceLock;

use crate::config::TranslationConfig;
use crate::log::*;
use crate::lyric_providers::{LyricLineOwned, LyricOwned};
use crate::sync::lyric::cache::refresh_lyric_cache;
use crate::sync::{LyricState, LYRIC, TRACK_PLAYING_STATE};

/// set once at startup from the `[translation]` table of `config.toml`
pub static TRANSLATION_CONFIG: OnceLock<TranslationConfig> = OnceLock::new();

/// lines per request: small enough for the first verses to show up quickly
const CHUNK_SIZE: usize = 8;

/// Translate the current lyric if it came without a translation.
///
/// `cache` tells whether the finished translation should be written back to
/// the lyric cache, so the song is not translated again next time.
pub async fn translate_missing(cache: bool) {
    let Some(config) = TRANSLATION_CONFIG.get() else {
        return;
    };

    let origin = LYRIC.with_borrow(|LyricState { origin, translation }| {
        match (origin, translation) {
            (LyricOwned::LineTimestamp(lines), LyricOwned::None | LyricOwned::NoTimestamp) => {
                Some(lines.clone())
            }
            _ => None,
        }
    });
    let Some(origin) = origin else {
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
                return;
            }
            Err(e) => {
                error!("translation task failed: {e}");
                return;
            }
        };

        let start = chunk_idx * CHUNK_SIZE;
        for (offset, line) in lines.into_iter().enumerate() {
            translated[start + offset] = Some(line);
        }

        let translation = origin
            .iter()
            .map(|line| LyricLineOwned {
                text: unique
                    .iter()
                    .position(|text| *text == line.text.trim())
                    .and_then(|idx| translated[idx].clone())
                    .unwrap_or_default(),
                start_time: line.start_time,
            })
            .collect();

        // the track may have changed while we were waiting for the backend
        let still_current = LYRIC.with_borrow_mut(|state| match &state.origin {
            LyricOwned::LineTimestamp(current) if *current == origin => {
                state.translation = LyricOwned::LineTimestamp(translation);
                true
            }
            _ => false,
        });
        if !still_current {
            debug!("track changed, dropping the rest of the translation");
            return;
        }
    }

    info!("translated {} distinct lines", unique.len());

    if cache {
        if let Some(cache_path) = TRACK_PLAYING_STATE.with_borrow(|state| state.cache_path.clone())
        {
            refresh_lyric_cache(&cache_path);
        }
    }
}
