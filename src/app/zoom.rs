//! Zooming the lyrics with `Alt` (or `Super`) + mouse wheel.
//!
//! Fonts are scaled with a Pango scale attribute on top of whatever sizes the
//! theme sets, so the proportions between the four rows are kept. In OSD mode
//! the bar is resized along with them and stays anchored at its bottom center,
//! so it grows upwards and sideways instead of sliding off the dock.

use std::time::Duration;

use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::ObjectSubclassIsExt;

use super::{get_labels, osd, Window, WINDOW_MIN_HEIGHT};
use crate::config;

pub const MIN_ZOOM: f64 = 0.5;
pub const MAX_ZOOM: f64 = 3.0;
/// zoom change per wheel notch
const STEP: f64 = 1.1;
/// how long the wheel has to rest before the zoom is written to the config
const SAVE_DELAY: Duration = Duration::from_millis(700);

pub fn scaled(size: i32, zoom: f64) -> i32 {
    (size as f64 * zoom).round() as i32
}

/// `Alt` or `Super` + wheel over the lyrics zooms them.
pub fn setup_scroll_zoom(window: &Window) {
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);

    scroll.connect_scroll({
        let window = window.downgrade();
        move |controller, _, dy| {
            if !super::move_modifier_held(controller.current_event_state()) {
                return glib::Propagation::Proceed;
            }
            let Some(window) = window.upgrade() else {
                return glib::Propagation::Proceed;
            };

            // wheel up = bigger; smooth scrolling gives fractional notches
            zoom_by(&window, STEP.powf(-dy));
            glib::Propagation::Stop
        }
    });

    window.add_controller(scroll);
}

/// Multiply the current zoom by `factor`, apply it and save it.
pub fn zoom_by(window: &Window, factor: f64) {
    let zoom = (window.imp().zoom.get() * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    apply(window, zoom);
    save_later(window, zoom);
}

/// Apply `zoom` to the labels and, in OSD mode, to the bar size.
pub fn apply(window: &Window, zoom: f64) {
    let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    window.imp().zoom.set(zoom);

    let attributes = pango::AttrList::new();
    attributes.insert(pango::AttrFloat::new_scale(zoom));
    if let Some(labels) = window.child().and_downcast::<gtk::Box>().as_ref().and_then(get_labels) {
        for label in labels {
            label.set_attributes(Some(&attributes));
        }
    }

    if window.has_css_class("osd-mode") {
        resize_osd(window, zoom);
    }
}

fn resize_osd(window: &Window, zoom: f64) {
    let (base_width, base_height) = window.imp().osd_base_size.get();
    let width = scaled(base_width, zoom);
    let height = scaled(base_height, zoom).max(WINDOW_MIN_HEIGHT);

    // the new size reaches `keep_anchored` through the next allocation
    window.set_default_size(width, height);
}

/// Keep the OSD bar's bottom center on its anchor whenever its size changes:
/// on zoom, and when a long line makes GTK widen the bar.
///
/// Called from the window's `size_allocate`.
pub fn keep_anchored(window: &Window, width: i32, height: i32) {
    let imp = window.imp();
    if !window.has_css_class("osd-mode") || imp.osd_dragging.get() {
        return;
    }
    if imp.osd_last_size.replace((width, height)) == (width, height) {
        return;
    }
    let Some((anchor_x, anchor_bottom)) = imp.osd_anchor.get() else {
        return;
    };

    osd::place(window, anchor_x - width / 2, anchor_bottom - height);
}

/// Remember the bottom center of the OSD bar placed at `(x, y)`.
pub fn set_anchor(window: &Window, x: i32, y: i32, width: i32, height: i32) {
    window
        .imp()
        .osd_anchor
        .set(Some((x + width / 2, y + height)));
}

/// Write the zoom to the config once the wheel has stopped turning.
fn save_later(window: &Window, zoom: f64) {
    let generation = window.imp().zoom_generation.get().wrapping_add(1);
    window.imp().zoom_generation.set(generation);

    let window = window.downgrade();
    glib::timeout_add_local_once(SAVE_DELAY, move || {
        let Some(window) = window.upgrade() else {
            return;
        };
        if window.imp().zoom_generation.get() != generation {
            return;
        }

        config::save_float("zoom", zoom);

        // a bar the user placed by hand keeps its exact spot across restarts;
        // an automatic one is placed from the zoomed size again at startup
        if window.imp().osd_pinned.get() {
            if let Some((x, y)) = osd::position(&window) {
                config::save_lyric_offset(x, y);
            }
        }
    });
}
