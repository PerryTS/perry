use gtk4::prelude::*;
use gtk4::{self, Orientation};

use perry_ffi::copy_string_from_raw as str_from_header;

/// Create a Form container (vertical box with extra padding).
pub fn create() -> i64 {
    crate::app::ensure_gtk_init();
    let vbox = gtk4::Box::new(Orientation::Vertical, 16);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);
    super::register_widget(vbox.upcast())
}

/// Create a Section with a title label (Frame with inner box).
pub fn section_create(title_ptr: *const u8) -> i64 {
    crate::app::ensure_gtk_init();
    let title = unsafe { str_from_header(title_ptr) };
    let frame = gtk4::Frame::new(Some(&title));
    let inner = gtk4::Box::new(Orientation::Vertical, 8);
    inner.set_margin_top(8);
    inner.set_margin_bottom(8);
    inner.set_margin_start(8);
    inner.set_margin_end(8);
    frame.set_child(Some(&inner));
    super::register_widget(frame.upcast())
}
