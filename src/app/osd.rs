//! Placement of the compact floating OSD bar.
//!
//! GTK4 dropped `gtk_window_move`, so a borderless window cannot place itself
//! through GTK anymore. LexLyrics runs on X11/XWayland by default (see
//! `GDK_BACKEND` in `main`, needed to dodge the GTK 4.18 DMABUF crash on
//! COSMIC), so the bar is made an unmanaged X11 window and the X server does
//! the placement instead.
//!
//! Everything here degrades to a no-op when the surface is not an X11 one, in
//! which case the window keeps whatever position the compositor gave it.

use crate::app::Window;
use crate::log::{debug, warn};

use gtk::prelude::*;

/// space left between the OSD bar and the bottom of the monitor, so that the
/// bar sits just above the COSMIC dock
pub const DEFAULT_DOCK_MARGIN: i32 = 64;

#[cfg(all(unix, not(target_os = "macos")))]
mod backend {
    use std::cell::Cell;
    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
    use std::ptr;

    /// `XSetWindowAttributes` from Xlib, only `override_redirect` is ever set
    #[repr(C)]
    #[derive(Default)]
    struct XSetWindowAttributes {
        background_pixmap: c_ulong,
        background_pixel: c_ulong,
        border_pixmap: c_ulong,
        border_pixel: c_ulong,
        bit_gravity: c_int,
        win_gravity: c_int,
        backing_store: c_int,
        backing_planes: c_ulong,
        backing_pixel: c_ulong,
        save_under: c_int,
        event_mask: c_long,
        do_not_propagate_mask: c_long,
        override_redirect: c_int,
        colormap: c_ulong,
        cursor: c_ulong,
    }

    const CW_OVERRIDE_REDIRECT: c_ulong = 1 << 9;

    // libX11 is already linked in by GTK's X11 backend; we only need a handful
    // of calls GDK does not wrap.
    #[link(name = "X11")]
    extern "C" {
        fn XOpenDisplay(name: *const c_char) -> *mut c_void;
        fn XChangeWindowAttributes(
            display: *mut c_void,
            w: c_ulong,
            valuemask: c_ulong,
            attributes: *mut XSetWindowAttributes,
        ) -> c_int;
        fn XSync(display: *mut c_void, discard: c_int) -> c_int;
        fn XDefaultRootWindow(display: *mut c_void) -> c_ulong;
        fn XMoveWindow(display: *mut c_void, w: c_ulong, x: c_int, y: c_int) -> c_int;
        fn XFlush(display: *mut c_void) -> c_int;
        fn XTranslateCoordinates(
            display: *mut c_void,
            src_w: c_ulong,
            dest_w: c_ulong,
            src_x: c_int,
            src_y: c_int,
            dest_x: *mut c_int,
            dest_y: *mut c_int,
            child: *mut c_ulong,
        ) -> c_int;
        fn XQueryPointer(
            display: *mut c_void,
            w: c_ulong,
            root: *mut c_ulong,
            child: *mut c_ulong,
            root_x: *mut c_int,
            root_y: *mut c_int,
            win_x: *mut c_int,
            win_y: *mut c_int,
            mask: *mut c_uint,
        ) -> c_int;
    }

    thread_local! {
        /// our own connection to the X server, opened on first use
        static DISPLAY: Cell<*mut c_void> = const { Cell::new(ptr::null_mut()) };
    }

    /// `Button1` in the modifier mask returned by `XQueryPointer`
    const BUTTON1_MASK: c_uint = 1 << 8;

    fn display() -> *mut c_void {
        DISPLAY.with(|cell| {
            let mut display = cell.get();
            if display.is_null() {
                display = unsafe { XOpenDisplay(ptr::null()) };
                cell.set(display);
            }
            display
        })
    }

    /// Take the window out of the window manager's hands. Must happen before
    /// the window is mapped for the first time.
    pub fn set_override_redirect(xid: u64) -> bool {
        let display = display();
        if display.is_null() {
            return false;
        }
        let mut attributes = XSetWindowAttributes {
            override_redirect: 1,
            ..Default::default()
        };
        unsafe {
            XChangeWindowAttributes(
                display,
                xid as c_ulong,
                CW_OVERRIDE_REDIRECT,
                &mut attributes,
            );
            // GTK maps the window through its own connection right after:
            // make sure the server has seen the attribute first
            XSync(display, 0);
        }
        true
    }

    pub fn move_to(xid: u64, x: i32, y: i32) -> bool {
        let display = display();
        if display.is_null() {
            return false;
        }
        unsafe {
            XMoveWindow(display, xid as c_ulong, x, y);
            XFlush(display);
        }
        true
    }

    /// absolute position of the window's top-left corner, in root coordinates
    pub fn absolute_position(xid: u64) -> Option<(i32, i32)> {
        let display = display();
        if display.is_null() {
            return None;
        }
        let mut x = 0;
        let mut y = 0;
        let mut child = 0;
        let ok = unsafe {
            let root = XDefaultRootWindow(display);
            XTranslateCoordinates(display, xid as c_ulong, root, 0, 0, &mut x, &mut y, &mut child)
        };
        (ok != 0).then_some((x, y))
    }

    /// pointer position in root coordinates, plus whether the left button is held
    pub fn pointer() -> Option<(i32, i32, bool)> {
        let display = display();
        if display.is_null() {
            return None;
        }
        let mut root = 0;
        let mut child = 0;
        let mut root_x = 0;
        let mut root_y = 0;
        let mut win_x = 0;
        let mut win_y = 0;
        let mut mask = 0;
        let ok = unsafe {
            let root_window = XDefaultRootWindow(display);
            XQueryPointer(
                display,
                root_window,
                &mut root,
                &mut child,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            )
        };
        (ok != 0).then_some((root_x, root_y, mask & BUTTON1_MASK != 0))
    }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
mod backend {
    pub fn set_override_redirect(_xid: u64) -> bool {
        false
    }
    pub fn move_to(_xid: u64, _x: i32, _y: i32) -> bool {
        false
    }
    pub fn absolute_position(_xid: u64) -> Option<(i32, i32)> {
        None
    }
    pub fn pointer() -> Option<(i32, i32, bool)> {
        None
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn xid(window: &Window) -> Option<u64> {
    let surface = window.surface()?;
    let surface = surface.downcast_ref::<gdk4_x11::X11Surface>()?;
    Some(surface.xid())
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn xid(_window: &Window) -> Option<u64> {
    None
}

/// Make the OSD bar an unmanaged (override-redirect) window, like the
/// `GTK_WINDOW_POPUP` the classic OSD lyrics apps used.
///
/// COSMIC's X11 window manager ignores client-requested moves of managed
/// windows, so this is what lets the bar sit exactly where it was left; as a
/// bonus it always stays above other windows. The trade-off is that the window
/// manager never gives it keyboard focus, so its keyboard shortcuts are not
/// available in OSD mode (the tray menu covers them).
///
/// Call it on a realized window that has not been shown yet.
pub fn make_unmanaged(window: &Window) -> bool {
    let Some(xid) = xid(window) else {
        warn!("not an X11 surface, the OSD bar stays managed by the compositor");
        return false;
    };
    backend::set_override_redirect(xid)
}

/// Move the whole window to `(x, y)` in root coordinates.
///
/// Returns `false` when the platform gives us no way to do it, so callers can
/// leave the placement to the compositor.
pub fn place(window: &Window, x: i32, y: i32) -> bool {
    let Some(xid) = xid(window) else {
        warn!("OSD placement needs an X11 surface, leaving the window where the compositor put it");
        return false;
    };
    debug!("moving OSD window to {x}x{y}");
    backend::move_to(xid, x.max(0), y.max(0))
}

/// Where the window currently is, in root coordinates.
pub fn position(window: &Window) -> Option<(i32, i32)> {
    backend::absolute_position(xid(window)?)
}

/// Pointer position in root coordinates and whether the left button is held.
pub fn pointer() -> Option<(i32, i32, bool)> {
    backend::pointer()
}

/// The monitor the OSD bar belongs on: the primary one when X11 knows it
/// (the bar has no monitor of its own before it is placed), otherwise the one
/// the window is on, otherwise the first.
fn target_monitor(window: &Window) -> Option<gtk::gdk::Monitor> {
    let display = WidgetExt::display(window);

    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(x11_display) = display.downcast_ref::<gdk4_x11::X11Display>() {
        // deprecated since GTK 4.18, but XRandR still has the notion
        #[allow(deprecated)]
        return Some(x11_display.primary_monitor());
    }

    window
        .surface()
        .and_then(|surface| display.monitor_at_surface(&surface))
        .or_else(|| display.monitors().item(0).and_downcast())
}

/// Bottom-centered placement on the primary monitor, keeping `dock_margin`
/// pixels of clearance for the dock.
pub fn bottom_center(window: &Window, width: i32, height: i32, dock_margin: i32) -> (i32, i32) {
    let geometry = target_monitor(window)
        .map(|monitor| monitor.geometry())
        .unwrap_or_else(|| gtk::gdk::Rectangle::new(0, 0, 1920, 1080));

    let x = geometry.x() + (geometry.width() - width).max(0) / 2;
    let y = geometry.y() + (geometry.height() - height - dock_margin).max(0);

    (x, y)
}
