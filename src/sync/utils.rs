use std::path::PathBuf;
use std::time::Duration;

use ahash::{HashMap, HashSet};
use gtk::subclass::prelude::*;

use crate::app;
use crate::log::*;
use crate::lyric_providers::{LyricLineOwned, SongInfo};

use super::{LyricState, TrackState, LYRIC, TRACK_PLAYING_STATE};

pub fn clean_lyric(window: &app::Window) {
    LYRIC.set(LyricState::default());
    window.imp().lyric_offset_ms.set(0);
}

/// Rank all candidates from a provider:
/// - Candidates matching track duration within `length_toleration_ms` come first
///   (weight `0` with fuzzy score when artist/album is known, `1` otherwise).
/// - Remaining candidates follow ordered by fuzzy match score (weight `2` if
///   fuzzy matched, `3` in original order).
pub fn rank_likely_lyrics<'a>(
    album: Option<&str>,
    title: &str,
    singer: Option<&str>,
    length: Option<Duration>,
    search_result: &'a [SongInfo],
    length_toleration_ms: u128,
) -> Vec<(&'a str, u8)> {
    if search_result.is_empty() {
        return Vec::new();
    }

    fn t2s(s: impl AsRef<str>) -> Vec<char> {
        zhconv::zhconv(s.as_ref(), zhconv::Variant::ZhHans)
            .chars()
            .collect()
    }

    let can_fuzzy = album.is_some() || singer.is_some();
    let o_title = can_fuzzy.then(|| t2s(title));
    let o_album = album.map(t2s);
    let o_singer = singer.map(t2s);

    let mut scored: Vec<(usize, &SongInfo, u8, i64)> = search_result
        .iter()
        .enumerate()
        .map(|(idx, song)| {
            let len_matches = length.is_some_and(|leng| {
                song.length.as_millis().abs_diff(leng.as_millis()) <= length_toleration_ms
            });
            let likelihood = if let Some(ref o_title) = o_title {
                let r_title = t2s(&song.title);
                let r_album = song.album.as_ref().map(t2s);
                let r_singer = t2s(&song.singer);
                let score = fuzzy_match_song(
                    o_title,
                    o_album.as_deref(),
                    o_singer.as_deref(),
                    &r_title,
                    r_album.as_deref(),
                    &r_singer,
                );
                if score.is_nan() {
                    0
                } else {
                    (score * 1024.0) as i64
                }
            } else {
                0
            };

            let weight = match (len_matches, can_fuzzy) {
                (true, true) => 0,
                (true, false) => 1,
                (false, true) => 2,
                (false, false) => 3,
            };
            trace!("rank w={weight} p={likelihood} for {song:?}");
            (idx, song, weight, likelihood)
        })
        .collect();

    scored.sort_by(|a, b| {
        a.2.cmp(&b.2)
            .then_with(|| b.3.cmp(&a.3))
            .then_with(|| a.0.cmp(&b.0))
    });

    scored
        .into_iter()
        .map(|(_, song, weight, _)| (song.id.as_str(), weight))
        .collect()
}

pub fn set_current_lyric(lyric: LyricState) {
    LYRIC.set(lyric);
    crate::sync::lyric::translate::set_refined(false);
}

pub fn get_lyric_cache_path() -> Option<PathBuf> {
    TRACK_PLAYING_STATE.with_borrow(|TrackState { cache_path, .. }| cache_path.as_ref().cloned())
}

pub fn fuzzy_match_song(
    title: &[char],
    album: Option<&[char]>,
    singer: Option<&[char]>,
    r_title: &[char],
    r_album: Option<&[char]>,
    r_singer: &[char],
) -> f64 {
    let title_likelihood = distance(title, r_title);
    let singer_likelihood = || distance(singer.unwrap_or_default(), r_singer);
    let album_likelihood = || distance(album.unwrap_or_default(), r_album.unwrap_or_default());
    match (singer, album) {
        (Some(_), Some(_)) => {
            title_likelihood * 0.4 + singer_likelihood() * 0.2 + album_likelihood() * 0.4
        }
        (Some(_), None) => title_likelihood * 0.7 + singer_likelihood() * 0.3,
        (None, Some(_)) => title_likelihood * 0.8 + album_likelihood() * 0.2,
        (None, None) => title_likelihood,
    }
}

pub fn extract_translated_lyric(lyric: impl AsRef<[LyricLineOwned]>) -> Vec<LyricLineOwned> {
    lyric
        .as_ref()
        .windows(2)
        .filter_map(|l| <&[LyricLineOwned; 2]>::try_from(l).ok())
        // this should work because we have sorted lyrics by timestamp
        .filter(|&[a, b]| a.start_time == b.start_time)
        .map(|p| &p[1])
        .cloned()
        .collect()
}

pub fn filter_original_lyric(
    lyric: impl AsRef<[LyricLineOwned]>,
    tlyric: impl AsRef<[LyricLineOwned]>,
) -> Vec<LyricLineOwned> {
    let tlyric = tlyric
        .as_ref()
        .iter()
        .map(|LyricLineOwned { text, start_time }| (start_time, text))
        .collect::<HashMap<_, _>>();
    let tlyric_lines = lyric
        .as_ref()
        .iter()
        .filter(|LyricLineOwned { text, start_time }| {
            !tlyric.contains_key(start_time) || tlyric[start_time] != text
        })
        .cloned()
        .collect::<Vec<_>>();
    tlyric_lines
}

pub fn distance<T: std::hash::Hash + Eq>(x: &[T], y: &[T]) -> f64 {
    let x_windows: HashSet<_> = x.windows(2).collect();
    let y_windows: HashSet<_> = y.windows(2).collect();

    let intersection = x_windows.intersection(&y_windows).count();

    2.0 * intersection as f64 / (x_windows.len() + y_windows.len()) as f64
}
