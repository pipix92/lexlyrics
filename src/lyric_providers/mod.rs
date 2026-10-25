mod strip_util;
pub use strip_util::strip_extended_timestamps;

pub mod utils;
use anyhow::Result;
use strum::EnumIs;

pub mod dict;

pub mod lrclib;
pub mod netease;
pub mod qqmusic;

use std::borrow::Cow;
use std::fmt::Debug;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum Lyric<'a> {
    None,
    NoTimestamp,
    LineTimestamp(Vec<LyricLine<'a>>),
}

#[derive(Debug)]
pub struct LyricLine<'a> {
    pub text: Cow<'a, str>,
    pub start_time: Duration,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Default, EnumIs)]
#[serde(tag = "type", content = "content")]
pub enum LyricOwned {
    #[default]
    None,
    NoTimestamp,
    LineTimestamp(Vec<LyricLineOwned>),
}

impl LyricOwned {
    /// a timed lyric with at least one non-blank line: providers sometimes
    /// answer with a timed lyric that has nothing in it
    pub fn has_lines(&self) -> bool {
        matches!(self, Self::LineTimestamp(lines) if lines.iter().any(|l| !l.text.trim().is_empty()))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct LyricLineOwned {
    pub text: String,
    pub start_time: Duration,
}

#[derive(Debug)]
pub struct SongInfo {
    pub id: String,
    pub title: String,
    pub singer: String,
    pub album: Option<String>,
    pub length: Duration,
}

#[async_trait::async_trait]
pub trait LyricProvider: LyricParse + Send + Sync {
    async fn query_lyric(&self, id: &str) -> Result<LyricStore>;
    async fn search_song_detailed(
        &self,
        album: &str,
        artists: &[&str],
        title: &str,
    ) -> Result<Vec<SongInfo>>;
    async fn search_song(&self, keyword: &str) -> Result<Vec<SongInfo>>;
    fn unique_name(&self) -> &'static str;
    fn init(self, config: &str) -> Result<()>;
    fn is_likely_songid(&self, s: &str) -> bool;
}

impl Debug for dyn LyricProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.unique_name())
    }
}

pub trait LyricParse {
    fn parse_lyric(&self, store: &LyricStore) -> LyricOwned;
    fn parse_translated_lyric(&self, store: &LyricStore) -> LyricOwned;
}

impl Lyric<'_> {
    pub fn into_owned(self) -> LyricOwned {
        match self {
            Lyric::None => LyricOwned::None,
            Lyric::NoTimestamp => LyricOwned::NoTimestamp,
            Lyric::LineTimestamp(lyrics) => LyricOwned::LineTimestamp(
                lyrics
                    .into_iter()
                    .map(LyricLine::<'_>::into_owned)
                    .collect(),
            ),
        }
    }
}

impl LyricLine<'_> {
    pub fn into_owned(Self { text, start_time }: Self) -> LyricLineOwned {
        LyricLineOwned {
            text: text.into(),
            start_time,
        }
    }
}

pub struct LyricStore {
    lyric: Option<String>,
    tlyric: Option<String>,
    /// the timestamps were estimated from plain lyrics, nobody synced them:
    /// only used when no provider has a synced version
    pub estimated: bool,
}

/// Turn plain lyrics into LRC by spreading the lines over the song, each one
/// getting time in proportion to its length, between a guessed intro and
/// outro. Blank lines (stanza breaks) become short pauses.
pub fn estimate_timing(plain: &str, duration_s: f64) -> String {
    let lines: Vec<&str> = plain.lines().map(str::trim).collect();
    let weight = |line: &str| line.chars().count() as f64 + 12.0;
    let total: f64 = lines.iter().map(|line| weight(line)).sum();
    if lines.iter().all(|line| line.is_empty()) || duration_s <= 0.0 {
        return String::new();
    }

    let (start, end) = (duration_s * 0.07, duration_s * 0.93);
    let mut time = start;
    let mut lrc = String::new();
    for line in lines {
        let minutes = (time / 60.0).floor();
        lrc += &format!("[{:02}:{:05.2}]{line}\n", minutes as u64, time - minutes * 60.0);
        time += (end - start) * weight(line) / total;
    }
    lrc
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no search result!")]
    NoResult,
}

pub fn default_search_query(album: &str, artists: &[&str], title: &str) -> String {
    format!("{title} {album} {}", artists.join("/"))
}

#[cfg(test)]
mod estimate_tests {
    use super::{estimate_timing, utils::lrc_iter};

    #[test]
    fn estimated_lrc_parses_in_order() {
        let plain = "First line of the song\nSecond one\n\nAfter a stanza break\nLast line";
        let lrc = estimate_timing(plain, 200.0);
        let lines = lrc_iter(lrc.lines()).expect("estimated lyric must be valid LRC");

        let texts: Vec<_> = lines.iter().map(|l| l.text.trim()).collect();
        assert_eq!(texts.iter().filter(|t| !t.is_empty()).count(), 4);
        assert!(lines.windows(2).all(|w| w[0].start_time < w[1].start_time));
        assert!(lines[0].start_time.as_secs_f64() >= 13.0);
        assert!(lines.last().unwrap().start_time.as_secs_f64() < 186.0);
    }

    #[test]
    fn nothing_to_estimate() {
        assert!(estimate_timing("\n\n", 200.0).is_empty());
        assert!(estimate_timing("a line", 0.0).is_empty());
    }
}
