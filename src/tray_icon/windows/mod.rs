use std::mem;
use std::process::Command;

use anyhow::Result;
use async_channel::Sender;

use tray_item::{IconSource, TrayItem};

use crate::app::actions::{UIAction, UI_ACTION};
use crate::sync::{PlayAction, PLAY_ACTION};
use crate::utils::gettext;

fn restart_self() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(&exe).spawn();
    }
    let _ = ui_action().send_blocking(UIAction::Quit);
}

fn set_window_mode(fullscreen: bool) {
    crate::config::save_bool("start-fullscreen", fullscreen);
    crate::config::save_lyric_offset(0, 0);
    restart_self();
}

pub fn start_tray_service() -> Result<()> {
    let icon = IconSource::Resource("icon0");
    let mut tray = TrayItem::new(crate::DEFAULT_TEXT, icon)?;

    tray.add_menu_item(&gettext("Compact OSD Bar"), || {
        set_window_mode(false);
    })?;
    tray.add_menu_item(&gettext("Fullscreen HUD"), || {
        set_window_mode(true);
    })?;
    tray.add_menu_item(&gettext("Bigger"), || {
        let _ = ui_action().send_blocking(UIAction::Zoom(1.1));
    })?;
    tray.add_menu_item(&gettext("Smaller"), || {
        let _ = ui_action().send_blocking(UIAction::Zoom(1.0 / 1.1));
    })?;
    tray.add_menu_item(&gettext("Move Back Above the Dock"), || {
        let _ = ui_action().send_blocking(UIAction::ResetPosition);
    })?;
    tray.add_menu_item(&gettext("Toggle Passthrough"), || {
        let _ = ui_action().send_blocking(UIAction::SwitchPassthrough);
    })?;
    tray.add_menu_item(&gettext("Search lyric"), || {
        let _ = play_action().send_blocking(PlayAction::SearchLyric);
    })?;
    tray.add_menu_item(&gettext("Refetch lyric"), || {
        let _ = play_action().send_blocking(PlayAction::RefetchLyric);
    })?;
    tray.add_menu_item(&gettext("Reload theme"), || {
        let _ = ui_action().send_blocking(UIAction::ReloadTheme);
    })?;
    tray.add_menu_item(&gettext("Restart"), restart_self)?;
    tray.add_menu_item(&gettext("Quit"), || {
        let _ = ui_action().send_blocking(UIAction::Quit);
    })?;

    mem::forget(tray);

    Ok(())
}

fn ui_action() -> &'static Sender<UIAction> {
    UI_ACTION.get().unwrap()
}
fn play_action() -> &'static Sender<PlayAction> {
    PLAY_ACTION.get().unwrap()
}
