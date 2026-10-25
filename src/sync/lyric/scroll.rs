use std::time::Duration;

use gtk::glib::{self, ControlFlow, Priority, WeakRef};
use gtk::subclass::prelude::ObjectSubclassIsExt;

use crate::app::{self, get_label};
use crate::config::LyricDisplayMode;
use crate::log::*;
use crate::lyric_providers::{LyricLineOwned, LyricOwned};

use crate::sync::{LyricState, TrackState, LYRIC, TRACK_PLAYING_STATE};
use crate::utils::reset_lyric_labels;

pub fn register_lyric_display(app: WeakRef<app::Window>, interval: Duration) {
    glib::timeout_add_local_full(interval, Priority::HIGH, move || {
        let Some(window) = app.upgrade() else {
            return ControlFlow::Break;
        };

        let (paused, metainfo_not_found) = TRACK_PLAYING_STATE.with_borrow(
            |TrackState {
                 metainfo, paused, ..
             }| (*paused, metainfo.is_none()),
        );

        if metainfo_not_found {
            return ControlFlow::Continue;
        }

        trace!("refresh lyric with paused = {paused}");
        refresh_lyric(&window, paused);

        ControlFlow::Continue
    });
}

fn set_lyric_with_mode(
    window: &app::Window,
    translation: Option<&LyricLineOwned>,
    origin: Option<&LyricLineOwned>,
) {
    let translation = {
        let respect_gap = window.imp().respect_empty_line_as_gap.get();
        let origin_is_empty = origin.is_some_and(|it| it.text.trim().is_empty());

        if respect_gap && origin_is_empty {
            None
        } else {
            translation
        }
    };

    match window.imp().lyric_display_mode.get() {
        LyricDisplayMode::ShowBoth => {
            set_lyric(window, translation.or(origin), "above");
            set_lyric(window, translation.and(origin), "below");
        }
        LyricDisplayMode::ShowBothRev => {
            set_lyric(window, origin, "above");
            set_lyric(window, translation, "below");
        }
        LyricDisplayMode::Origin => {
            set_lyric(window, origin, "above");
            set_lyric(window, None, "below");
        }
        LyricDisplayMode::PreferTranslation => {
            set_lyric(window, translation.or(origin), "above");
            set_lyric(window, None, "below");
        }
    }
}

fn set_lyric(window: &app::Window, text: Option<&LyricLineOwned>, position: &str) {
    let text = text
        .map(|LyricLineOwned { text, .. }| text.as_str().trim())
        .unwrap_or_default();

    get_label(window, position).set_label(text);
}

/// Before the first sung line, show what is playing: title on the main row,
/// artist on the translation row and the first line as a preview.
///
/// Returns `false` once the song has reached its first non-blank line.
fn show_intro(window: &app::Window, lines: &[LyricLineOwned], elapsed: Duration) -> bool {
    let Some(first) = lines.iter().find(|line| !line.text.trim().is_empty()) else {
        return false;
    };
    if elapsed >= first.start_time {
        return false;
    }

    let (title, artists) = TRACK_PLAYING_STATE.with_borrow(|TrackState { metainfo, .. }| {
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

    get_label(window, "prev").set_label("");
    get_label(window, "above").set_label(&title);
    get_label(window, "below").set_label(&artists);
    let first_idx = lines.iter().position(|line| std::ptr::eq(line, first)).unwrap_or(0);
    let count = context_lines(window);
    get_label(window, "next").set_label(&join_context(
        lines[first_idx..].iter().take(count),
    ));
    true
}

/// rough height of the two main rows, and of one line above plus one below,
/// at zoom 1.0 with the karaoke theme
const MAIN_ROWS_HEIGHT: f64 = 110.0;
const CONTEXT_PAIR_HEIGHT: f64 = 60.0;
const MAX_CONTEXT_LINES: usize = 8;

/// How many lines to show before and after the current one: the configured
/// number, or with `context-lines = -1` as many as the window has room for.
fn context_lines(window: &app::Window) -> usize {
    let configured = window.imp().context_lines.get();
    if configured >= 0 {
        return configured as usize;
    }

    let zoom = window.imp().zoom.get().max(0.1);
    let height = gtk::prelude::WidgetExt::height(window) as f64 / zoom;
    let fit = ((height - MAIN_ROWS_HEIGHT) / CONTEXT_PAIR_HEIGHT).floor();
    (fit.max(1.0) as usize).min(MAX_CONTEXT_LINES)
}

fn join_context<'a>(lines: impl Iterator<Item = &'a LyricLineOwned>) -> String {
    lines
        .map(|line| line.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Fill the grey rows around the current line with up to `context_lines`
/// non-blank lines each, the nearest ones next to the current line.
fn set_context(window: &app::Window, lines: &[LyricLineOwned], current_idx: usize) {
    let count = context_lines(window);
    let not_blank = |line: &&LyricLineOwned| !line.text.trim().is_empty();

    let mut before: Vec<_> = lines[..current_idx.min(lines.len())]
        .iter()
        .rev()
        .filter(not_blank)
        .take(count)
        .collect();
    before.reverse();
    let after = lines
        .iter()
        .skip(current_idx + 1)
        .filter(not_blank)
        .take(count);

    get_label(window, "prev").set_label(&join_context(before.into_iter()));
    get_label(window, "next").set_label(&join_context(after));
}

pub fn refresh_lyric(window: &app::Window, paused: bool) {
    if paused {
        if !window.imp().show_lyric_on_pause.get() {
            reset_lyric_labels(window, Some(""));
        }
        return;
    }

    LYRIC.with_borrow(
        |LyricState {
             origin,
             translation,
         }| {
            let system_time = window.imp().lyric_start.get().unwrap();
            let elapsed = system_time.elapsed().ok();
            let Some(elapsed) = elapsed else {
                return;
            };
            match (origin, translation) {
                (
                    LyricOwned::LineTimestamp(origin_lyric),
                    LyricOwned::LineTimestamp(translation_lyric),
                ) => {
                    if show_intro(window, origin_lyric, elapsed) {
                        return;
                    }

                    let current_idx = origin_lyric
                        .iter()
                        .position(|l| l.start_time > elapsed)
                        .unwrap_or(origin_lyric.len())
                        .saturating_sub(1);

                    let curr = origin_lyric.get(current_idx);

                    let translation =
                        crate::lyric_providers::utils::find_next_lyric(&elapsed, translation_lyric);

                    set_context(window, origin_lyric, current_idx);
                    set_lyric_with_mode(window, translation, curr);
                }
                (LyricOwned::LineTimestamp(origin_lyric), _) => {
                    if show_intro(window, origin_lyric, elapsed) {
                        return;
                    }

                    let current_idx = origin_lyric
                        .iter()
                        .position(|l| l.start_time > elapsed)
                        .unwrap_or(origin_lyric.len())
                        .saturating_sub(1);

                    let curr = origin_lyric.get(current_idx);

                    set_context(window, origin_lyric, current_idx);
                    set_lyric_with_mode(window, None, curr);
                }
                _ => (),
            }
        },
    );
}

