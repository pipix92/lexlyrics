pub mod search_window;
mod window;

use gtk::prelude::*;
use gtk::subclass::prelude::ObjectSubclassIsExt;
use gtk::{Application, Label};
pub use window::Window;

use crate::app::utils::set_click_pass_through;
use crate::app::utils::set_skip_taskbar;
use crate::{config, DEFAULT_TEXT};

const WINDOW_MIN_HEIGHT: i32 = 120;

pub mod actions;
pub mod dialog;
pub mod utils;

pub fn build_main_window(
    app: &Application,
    enable_filter_regex: bool,
    cache_lyrics: bool,
    length_toleration_ms: u128,
    show_default_text_on_idle: bool,
    show_lyric_on_pause: bool,
    respect_empty_line_as_gap: bool,
    skip_auto_search: bool,
    skip_taskbar: bool,
    #[cfg(feature = "layer-shell")] layer_shell: bool,
    #[cfg(feature = "layer-shell")] layer_shell_anchor: crate::config::LayerShellAnchor,
    start_fullscreen: bool,
    window_width: Option<i32>,
    window_height: Option<i32>,
    lyric_offset_x: i32,
    lyric_offset_y: i32,
    passthrough_by_default: bool,
) -> Window {
    let window = Window::new(
        app,
        cache_lyrics,
        length_toleration_ms,
        show_default_text_on_idle,
        show_lyric_on_pause,
        respect_empty_line_as_gap,
        skip_auto_search,
    );

    #[cfg(feature = "layer-shell")]
    if layer_shell {
        use gtk4_layer_shell::{is_supported, KeyboardMode, Layer, LayerShell};

        if is_supported() {
            LayerShell::init_layer_shell(&window);
            LayerShell::set_keyboard_mode(&window, KeyboardMode::OnDemand);
            LayerShell::set_layer(&window, Layer::Overlay);
        } else {
            tracing::warn!("layer-shell was enabled but unsupported by the compositor!");
        }

        layer_shell_anchor.setup(&window);
    }

    if start_fullscreen {
        window.fullscreen();
        window.set_decorated(false);
    } else if let (Some(w), Some(h)) = (window_width, window_height) {
        window.set_default_size(w, h);
    } else {
        window.set_size_request(500, WINDOW_MIN_HEIGHT);
    }

    window.set_title(Some(DEFAULT_TEXT));
    window.set_icon_name(Some(crate::APP_ID_FIXED));

    if skip_taskbar {
        gtk::prelude::WidgetExt::realize(&window);
        set_skip_taskbar(&window, true);
    }

    window.present();

    let prev_label = Label::builder()
        .label("")
        .name("prev")
        .vexpand(false)
        .hexpand(false)
        .build();
    let above_label = Label::builder()
        .label("LexLyrics")
        .name("above")
        .vexpand(true)
        .hexpand(false)
        .build();
    let below_label = Label::builder()
        .label("")
        .name("below")
        .vexpand(true)
        .hexpand(false)
        .visible(false)
        .build();
    let next_label = Label::builder()
        .label("")
        .name("next")
        .vexpand(false)
        .hexpand(false)
        .build();

    for label in [&prev_label, &above_label, &below_label, &next_label] {
        utils::setup_label(label, enable_filter_regex);
    }

    let verical_box = gtk::Box::builder()
        .name("lyrics-box")
        .baseline_position(gtk::BaselinePosition::Center)
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::End)
        .margin_bottom(52)
        .vexpand(true)
        .hexpand(false)
        .build();

    verical_box.insert_child_after(&prev_label, gtk::Box::NONE);
    verical_box.insert_child_after(&above_label, Some(&prev_label));
    verical_box.insert_child_after(&below_label, Some(&above_label));
    verical_box.insert_child_after(&next_label, Some(&below_label));

    if lyric_offset_x > 0 || lyric_offset_y > 0 {
        verical_box.set_halign(gtk::Align::Start);
        verical_box.set_valign(gtk::Align::Start);
        verical_box.set_margin_start(lyric_offset_x);
        verical_box.set_margin_top(lyric_offset_y);
    }

    // Alt + Mouse Drag to move lyrics-box freely anywhere on screen
    let drag = gtk::GestureDrag::new();
    let vbox_weak = verical_box.downgrade();
    let start_pos = std::rc::Rc::new(std::cell::Cell::new((0i32, 0i32)));
    let start_pos_clone = start_pos.clone();
    let vbox_weak_clone = vbox_weak.clone();

    drag.connect_drag_begin(move |gesture, _start_x, _start_y| {
        if let Some(event) = gesture.current_event() {
            if event.modifier_state().contains(gtk::gdk::ModifierType::ALT_MASK) {
                if let Some(vbox) = vbox_weak_clone.upgrade() {
                    start_pos_clone.set((vbox.margin_start(), vbox.margin_top()));
                }
            }
        }
    });

    drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        if let Some(event) = gesture.current_event() {
            if event.modifier_state().contains(gtk::gdk::ModifierType::ALT_MASK) {
                if let Some(vbox) = vbox_weak.upgrade() {
                    let (init_x, init_y) = start_pos.get();
                    vbox.set_halign(gtk::Align::Start);
                    vbox.set_valign(gtk::Align::Start);
                    let new_x = (init_x + offset_x as i32).max(0);
                    let new_y = (init_y + offset_y as i32).max(0);
                    vbox.set_margin_start(new_x);
                    vbox.set_margin_top(new_y);
                }
            }
        }
    });

    verical_box.add_controller(drag);
    window.set_child(Some(&verical_box));

    let align = window.imp().lyric_align.get();
    set_lyric_align(&window, align);

    window.connect_decorated_notify(|window| {
        crate::log::debug!("triggered decorated signal");
        let clickthrough = window.imp().clickthrough.get();
        set_click_pass_through(window, clickthrough)
    });

    if passthrough_by_default {
        window.imp().clickthrough.set(true);
        set_click_pass_through(&window, true);
    }

    window.set_icon_name(Some(crate::APP_ID_FIXED));

    window
}

pub fn set_lyric_align(window: &Window, align: config::Align) -> Option<()> {
    let vbox: gtk::Box = window.child()?.downcast().ok()?;
    vbox.set_halign(align.into());

    let labels = get_labels(&vbox)?;
    for label in labels {
        label.set_halign(align.into());
        label.set_justify(align.into());
    }

    window.imp().lyric_align.set(align);
    Some(())
}

fn get_labels(vbox: &gtk::Box) -> Option<[Label; 4]> {
    let mut labels = Vec::new();
    let mut child = vbox.first_child();
    while let Some(c) = child {
        child = c.next_sibling();
        if let Ok(label) = c.downcast::<Label>() {
            labels.push(label);
        }
    }
    if labels.len() == 4 {
        Some([labels[0].clone(), labels[1].clone(), labels[2].clone(), labels[3].clone()])
    } else {
        None
    }
}

pub fn get_label(window: &Window, position: &str) -> Label {
    let Some(vbox) = window.child().and_then(|c| c.downcast::<gtk::Box>().ok()) else {
        panic!("initialization failed!")
    };
    get_labels(&vbox)
        .expect("cannot find labels")
        .into_iter()
        .find(|label| label.widget_name() == position)
        .unwrap_or_else(|| panic!("cannot find label for position {position}"))
}

