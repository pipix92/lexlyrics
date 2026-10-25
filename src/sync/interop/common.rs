use anyhow::Error;
use gtk::subclass::prelude::*;
use tokio::sync::Mutex;

use std::sync::LazyLock;
use std::time::Duration;

use gtk::glib::{self, WeakRef};

use anyhow::Result;

use crate::app::{self, Window};
use crate::log::*;
use crate::sync::interop::{OsImp, PlayerStatus, OS};
use crate::sync::lyric::cache::{self, get_cache_path};
use crate::sync::lyric::fetch;
use crate::sync::utils::clean_lyric;
use crate::sync::{TrackMeta, TrackState, TRACK_PLAYING_STATE};
use crate::utils::reset_lyric_labels;

pub async fn update_lyric(
    track_meta: &TrackMeta,
    window: &app::Window,
    ignore_cache: bool,
) -> Result<(), Error> {
    static UPDATE_LYRIC_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
    // wait for the previous track's fetch instead of dropping this one: when
    // skipping quickly, the track that ends up playing would otherwise never
    // get its lyric
    let _gaurd = UPDATE_LYRIC_LOCK.lock().await;
    if !is_current_track(track_meta) {
        debug!("track changed while waiting for the previous fetch, skipping");
        return Ok(());
    }

    crate::sync::utils::clean_lyric(window);

    let cache_lyrics = window.imp().cache_lyrics.get();
    if cache_lyrics {
        cache::fetch_lyric_cached(track_meta, ignore_cache, window).await?
    } else {
        fetch::fetch_lyric(track_meta, window).await?
    };

    drop(_gaurd);

    // outside the lock: translating a whole song takes a while, and the next
    // track must not wait for it
    glib::spawn_future_local(crate::sync::lyric::translate::translate_missing(cache_lyrics));

    Ok(())
}

pub fn register_sync_task(wind: WeakRef<Window>, interval: Duration, auto_connect: bool) {
    glib::timeout_add_local(interval, move || {
        let Some(window) = wind.upgrade() else {
            return glib::ControlFlow::Continue;
        };

        match OS::try_sync_track(&window) {
            Err(PlayerStatus::Missing) => {
                TRACK_PLAYING_STATE.take();

                if auto_connect {
                    reset_lyric_labels(&window, None);
                    clean_lyric(&window);
                    OS::reconnect_player();
                }
            }
            Err(PlayerStatus::Unsupported(kind)) => {
                app::get_label(&window, "above").set_label("Unsupported Player");
                app::get_label(&window, "below").set_label(kind);

                clean_lyric(&window);
                error!(kind);
            }
            Err(PlayerStatus::Paused) => {
                TRACK_PLAYING_STATE.with_borrow_mut(|TrackState { paused, .. }| *paused = true)
            }
            Err(PlayerStatus::Stopped) => {
                reset_lyric_labels(&window, None);
                clean_lyric(&window);
                TRACK_PLAYING_STATE.take();
            }
            _ => TRACK_PLAYING_STATE.with_borrow_mut(|TrackState { paused, .. }| *paused = false),
        }

        glib::ControlFlow::Continue
    });
}

/// Whether `track_meta` is still the track playing (length is not compared,
/// see `need_fetch_lyric`).
pub fn is_current_track(track_meta: &TrackMeta) -> bool {
    TRACK_PLAYING_STATE.with_borrow(|TrackState { metainfo, .. }| {
        metainfo.as_ref().is_some_and(|playing| {
            TrackMeta {
                length: None,
                ..playing.clone()
            } == TrackMeta {
                length: None,
                ..track_meta.clone()
            }
        })
    })
}

pub fn need_fetch_lyric(track_meta: &TrackMeta) -> bool {
    TRACK_PLAYING_STATE.with_borrow_mut(
        |TrackState {
             metainfo,
             cache_path,
             ..
         }| {
            let track_meta_playing = metainfo.as_ref().cloned();
            trace!("got track_id: {track_meta:#?}");

            // ? issue [#109](https://github.com/waylyrics/waylyrics/issues/109)
            // skip comparing length
            let need = !track_meta_playing.is_some_and(|p| {
                TrackMeta { length: None, ..p }
                    == TrackMeta {
                        length: None,
                        ..track_meta.clone()
                    }
            });

            if need {
                *metainfo = Some(track_meta.clone());
                *cache_path = get_cache_path(track_meta);
            }
            need
        },
    )
}
