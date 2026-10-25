pub mod search_window;
mod window;

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::subclass::prelude::ObjectSubclassIsExt;
use gtk::{Application, Label};
pub use window::Window;

use crate::app::utils::set_click_pass_through;
use crate::app::utils::set_skip_taskbar;
use crate::{config, DEFAULT_TEXT};

const WINDOW_MIN_HEIGHT: i32 = 120;

/// fallback size of the compact OSD bar when the config carries none
const OSD_DEFAULT_SIZE: (i32, i32) = (1200, 210);

pub mod actions;
pub mod dialog;
pub mod osd;
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
    osd_dock_margin: i32,
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

    // Two window modes: the compact OSD bar (default) is a small borderless
    // strip living above the dock, the fullscreen HUD covers the whole screen
    // and places the lyrics inside it.
    let (osd_width, osd_height) = (
        window_width.unwrap_or(OSD_DEFAULT_SIZE.0),
        window_height.unwrap_or(OSD_DEFAULT_SIZE.1).max(WINDOW_MIN_HEIGHT),
    );

    if start_fullscreen {
        window.add_css_class("hud-mode");
        window.fullscreen();
        window.set_decorated(false);
    } else {
        window.add_css_class("osd-mode");
        window.set_decorated(false);
        window.set_default_size(osd_width, osd_height);
        window.set_size_request(320, WINDOW_MIN_HEIGHT);
    }

    window.set_title(Some(DEFAULT_TEXT));
    window.set_icon_name(Some(crate::APP_ID_FIXED));

    if skip_taskbar {
        gtk::prelude::WidgetExt::realize(&window);
        set_skip_taskbar(&window, true);
    }

    if !start_fullscreen {
        gtk::prelude::WidgetExt::realize(&window);
        osd::make_unmanaged(&window);
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
        // the HUD keeps the lyrics clear of the dock on its own, the OSD bar
        // is already sitting exactly where it should
        .valign(if start_fullscreen {
            gtk::Align::End
        } else {
            gtk::Align::Center
        })
        .margin_bottom(if start_fullscreen { 52 } else { 4 })
        .halign(gtk::Align::Center)
        .vexpand(true)
        .hexpand(false)
        .build();

    verical_box.insert_child_after(&prev_label, gtk::Box::NONE);
    verical_box.insert_child_after(&above_label, Some(&prev_label));
    verical_box.insert_child_after(&below_label, Some(&above_label));
    verical_box.insert_child_after(&next_label, Some(&below_label));

    window.set_child(Some(&verical_box));

    if start_fullscreen {
        if lyric_offset_x > 0 || lyric_offset_y > 0 {
            verical_box.set_halign(gtk::Align::Start);
            verical_box.set_valign(gtk::Align::Start);
            verical_box.set_margin_start(lyric_offset_x);
            verical_box.set_margin_top(lyric_offset_y);
        }
        setup_hud_drag(&verical_box);
    } else {
        place_osd_window(
            &window,
            osd_width,
            osd_height,
            osd_dock_margin,
            lyric_offset_x,
            lyric_offset_y,
        );
        setup_osd_drag(&window);
    }

    let align = window.imp().lyric_align.get();
    set_lyric_align(&window, align);

    window.connect_decorated_notify(|window| {
        crate::log::debug!("triggered decorated signal");
        let clickthrough = window.imp().clickthrough.get();
        set_click_pass_through(window, clickthrough)
    });

    // config wins over the state GSettings remembered from the last session:
    // `size_allocate` re-applies this flag on every allocation, so a stale
    // `true` there would leave the window impossible to grab or drag
    window.imp().clickthrough.set(passthrough_by_default);
    set_click_pass_through(&window, passthrough_by_default);
    window.imp().osd_dock_margin.set(osd_dock_margin);

    window.set_icon_name(Some(crate::APP_ID_FIXED));

    window
}

/// Put the OSD bar where the user left it, or just above the dock the first
/// time around.
fn place_osd_window(
    window: &Window,
    width: i32,
    height: i32,
    dock_margin: i32,
    saved_x: i32,
    saved_y: i32,
) {
    let target = if saved_x > 0 || saved_y > 0 {
        (saved_x, saved_y)
    } else {
        osd::bottom_center(window, width, height, dock_margin)
    };

    osd::place(window, target.0, target.1);
}

fn alt_held(gesture: &gtk::GestureDrag) -> bool {
    gesture.current_event().is_some_and(|event| {
        event
            .modifier_state()
            .contains(gtk::gdk::ModifierType::ALT_MASK)
    })
}

/// `Alt` + mouse drag moves the whole OSD bar around; where it lands is
/// written back to `config.toml`.
///
/// The position is recomputed from the pointer on every update instead of
/// accumulating the gesture's offsets, because the window moves out from under
/// the pointer as we go.
fn setup_osd_drag(window: &Window) {
    let drag = gtk::GestureDrag::new();
    // pointer offset inside the window when the drag started
    let grab = Rc::new(Cell::new(None::<(i32, i32)>));

    drag.connect_drag_begin({
        let grab = grab.clone();
        let window = window.downgrade();
        move |gesture, _, _| {
            grab.set(None);

            if !alt_held(gesture) {
                return;
            }
            let Some(window) = window.upgrade() else {
                return;
            };
            let (Some((pointer_x, pointer_y, _)), Some((window_x, window_y))) =
                (osd::pointer(), osd::position(&window))
            else {
                return;
            };

            grab.set(Some((pointer_x - window_x, pointer_y - window_y)));
        }
    });

    drag.connect_drag_update({
        let grab = grab.clone();
        let window = window.downgrade();
        move |_, _, _| {
            let (Some((grab_x, grab_y)), Some(window)) = (grab.get(), window.upgrade()) else {
                return;
            };
            let Some((pointer_x, pointer_y, _)) = osd::pointer() else {
                return;
            };

            osd::place(&window, pointer_x - grab_x, pointer_y - grab_y);
        }
    });

    drag.connect_drag_end({
        let window = window.downgrade();
        move |_, _, _| {
            let (Some(_), Some(window)) = (grab.take(), window.upgrade()) else {
                return;
            };

            if let Some((x, y)) = osd::position(&window) {
                config::save_lyric_offset(x, y);
            }
        }
    });

    window.add_controller(drag);
}

/// `Alt` + mouse drag moves the lyrics inside the fullscreen HUD, and the
/// offsets are written back to `config.toml`.
fn setup_hud_drag(lyrics_box: &gtk::Box) {
    let drag = gtk::GestureDrag::new();
    let start = Rc::new(Cell::new((0i32, 0i32)));

    drag.connect_drag_begin({
        let start = start.clone();
        let lyrics_box = lyrics_box.downgrade();
        move |gesture, _, _| {
            if !alt_held(gesture) {
                return;
            }
            if let Some(lyrics_box) = lyrics_box.upgrade() {
                start.set((lyrics_box.margin_start(), lyrics_box.margin_top()));
            }
        }
    });

    drag.connect_drag_update({
        let start = start.clone();
        let lyrics_box = lyrics_box.downgrade();
        move |gesture, offset_x, offset_y| {
            if !alt_held(gesture) {
                return;
            }
            let Some(lyrics_box) = lyrics_box.upgrade() else {
                return;
            };

            let (init_x, init_y) = start.get();
            lyrics_box.set_halign(gtk::Align::Start);
            lyrics_box.set_valign(gtk::Align::Start);
            lyrics_box.set_margin_start((init_x + offset_x as i32).max(0));
            lyrics_box.set_margin_top((init_y + offset_y as i32).max(0));
        }
    });

    drag.connect_drag_end({
        let lyrics_box = lyrics_box.downgrade();
        move |gesture, _, _| {
            if !alt_held(gesture) {
                return;
            }
            if let Some(lyrics_box) = lyrics_box.upgrade() {
                config::save_lyric_offset(lyrics_box.margin_start(), lyrics_box.margin_top());
            }
        }
    });

    lyrics_box.add_controller(drag);
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

