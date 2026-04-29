use gtk4::prelude::*;
use gtk4::Orientation;
use gtk4::Separator;

/// Create a horizontal separator line.
pub fn create() -> i64 {
    crate::app::ensure_gtk_init();
    let separator = Separator::new(Orientation::Horizontal);
    super::register_widget(separator.upcast())
}
