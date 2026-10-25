use std::sync::OnceLock;

use async_channel::Sender;
use gtk::glib::{self, WeakRef};
use gtk::prelude::*;
use gtk::Application;

use crate::app::Window;
use crate::log::debug;

#[derive(Clone, Debug)]
pub enum UIAction {
    /// set label content for above label
    SetAboveLabel(String),
    /// set label content for below label
    SetBelowLabel(String),
    ReloadTheme,
    /// toggles mouse click passthrough
    SwitchPassthrough,
    /// toggles GTK+ CSD
    SwitchDecoration,
    /// set new lyric display mode
    SetDisplayMode(String),
    /// set new lyric alignment mode
    SetLyricAlign(String),
    /// put the lyrics back above the dock and forget the dragged position
    ResetPosition,
    /// multiply the lyrics zoom by this factor
    Zoom(f64),
    /// grey lines around the current one, -1 = as many as fit
    SetContextLines(i32),
    /// Quit Waylyrics
    Quit,
}

fn register_ui_action(app: WeakRef<Application>, wind: WeakRef<Window>) -> Sender<UIAction> {
    let (tx, rx) = async_channel::unbounded();

    glib::spawn_future_local(async move {
        while let Ok(action) = rx.recv().await {
            debug!("Received UI Action {action:?}");

            let Some(app) = app.upgrade() else {
                continue;
            };
            let Some(wind) = wind.upgrade() else {
                continue;
            };

            match action {
                UIAction::SetAboveLabel(text) => ActionGroupExt::activate_action(
                    &wind,
                    "set-label",
                    Some(&vec!["above".to_string(), text].to_variant()),
                ),
                UIAction::SetBelowLabel(text) => ActionGroupExt::activate_action(
                    &wind,
                    "set-label",
                    Some(&vec!["below".to_string(), text].to_variant()),
                ),
                UIAction::ReloadTheme => app.activate_action("reload-theme", None),
                UIAction::SwitchPassthrough => {
                    ActionGroupExt::activate_action(&wind, "switch-passthrough", None)
                }
                UIAction::SwitchDecoration => {
                    ActionGroupExt::activate_action(&wind, "switch-decoration", None)
                }
                UIAction::SetDisplayMode(display_mode) => ActionGroupExt::activate_action(
                    &wind,
                    "set-display-mode",
                    Some(&display_mode.to_variant()),
                ),
                UIAction::SetLyricAlign(lyric_align) => ActionGroupExt::activate_action(
                    &wind,
                    "set-lyric-align",
                    Some(&lyric_align.to_variant()),
                ),
                UIAction::ResetPosition => {
                    ActionGroupExt::activate_action(&wind, "reset-position", None)
                }
                UIAction::SetContextLines(count) => ActionGroupExt::activate_action(
                    &wind,
                    "set-context-lines",
                    Some(&count.to_variant()),
                ),
                UIAction::Zoom(factor) => {
                    ActionGroupExt::activate_action(&wind, "zoom", Some(&factor.to_variant()))
                }
                UIAction::Quit => {
                    wind.close();
                }
            }
        }
    });

    tx
}

pub fn init_ui_action_channel(app: WeakRef<Application>, wind: WeakRef<Window>) {
    let tx = register_ui_action(app, wind);
    UI_ACTION.set(tx).expect("must only initialize once");
}

pub static UI_ACTION: OnceLock<Sender<UIAction>> = OnceLock::new();
