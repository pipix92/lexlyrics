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
                    let current_idx = origin_lyric
                        .iter()
                        .position(|l| l.start_time > elapsed)
                        .unwrap_or(origin_lyric.len())
                        .saturating_sub(1);

                    let prev = if current_idx > 0 { origin_lyric.get(current_idx - 1) } else { None };
                    let curr = origin_lyric.get(current_idx);
                    let next = origin_lyric.get(current_idx + 1);

                    let translation =
                        crate::lyric_providers::utils::find_next_lyric(&elapsed, translation_lyric);

                    set_lyric(window, prev, "prev");
                    set_lyric_with_mode(window, translation, curr);
                    set_lyric(window, next, "next");
                }
                (LyricOwned::LineTimestamp(origin_lyric), _) => {
                    let current_idx = origin_lyric
                        .iter()
                        .position(|l| l.start_time > elapsed)
                        .unwrap_or(origin_lyric.len())
                        .saturating_sub(1);

                    let prev = if current_idx > 0 { origin_lyric.get(current_idx - 1) } else { None };
                    let curr = origin_lyric.get(current_idx);
                    let next = origin_lyric.get(current_idx + 1);

                    set_lyric(window, prev, "prev");
                    set_lyric_with_mode(window, None, curr);
                    set_lyric(window, next, "next");
                }
                _ => (),
            }
        },
    );
}

