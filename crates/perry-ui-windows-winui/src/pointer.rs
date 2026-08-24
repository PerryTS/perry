//! Generic pointer callback dispatch for Fluent widgets.

use crate::winui::backend::{self, RenderBackend};

pub fn set_on_click(handle: i64, callback: f64) {
    if backend::active() == RenderBackend::Fluent {
        crate::widgets::set_on_click(handle, callback);
    } else {
        perry_ui_windows::pointer::set_on_click(handle, callback);
    }
}

pub fn set_on_mouse_down(handle: i64, callback: f64) {
    perry_ui_windows::pointer::set_on_mouse_down(handle, callback);
}

pub fn set_on_mouse_up(handle: i64, callback: f64) {
    perry_ui_windows::pointer::set_on_mouse_up(handle, callback);
}

pub fn set_on_mouse_move(handle: i64, callback: f64) {
    perry_ui_windows::pointer::set_on_mouse_move(handle, callback);
}

pub fn set_on_hover(handle: i64, callback: f64) {
    perry_ui_windows::pointer::set_on_hover(handle, callback);
}
