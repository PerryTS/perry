use gtk4::prelude::*;
use gtk4::{Orientation, Paned};

/// Create a horizontal split-pane container. Mirrors macOS
/// `perry_ui_splitview_create` (`NSSplitView`). `_left_width` is accepted
/// for signature parity with the macOS twin but is not honored — GTK4's
/// `Paned` exposes `set_position(i32)` for the divider, which represents
/// "pixels from the start child", and we let the user call that
/// separately via the existing widget setters if they need it. Default
/// orientation is horizontal (vertical divider, panes laid out
/// side-by-side), matching the macOS default.
///
/// **Caveat (vs macOS):** GTK4 `Paned` supports exactly **two** children
/// (`start_child` + `end_child`). `NSSplitView` supports N. Use nested
/// `SplitView`s to express more than two panes on Linux, or call
/// `splitview_add_child` at most twice — the third+ call is a no-op with
/// a warning.
pub fn create(_left_width: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let paned = Paned::new(Orientation::Horizontal);
    super::register_widget(paned.upcast())
}

/// Add a child to a Paned. First call → `set_start_child`, second call →
/// `set_end_child`, third+ call → no-op + warning. The macOS twin takes
/// an `index` arg; on GTK4 we ignore it because `Paned` only has two
/// fixed slots — child order is determined by call order.
pub fn add_child(parent_handle: i64, child_handle: i64, _index: i64) {
    if let (Some(parent), Some(child)) = (
        super::get_widget(parent_handle),
        super::get_widget(child_handle),
    ) {
        let Some(paned) = parent.downcast_ref::<Paned>() else {
            eprintln!("perry-ui-gtk4: splitview_add_child called on non-Paned parent");
            return;
        };
        if child.parent().is_some() {
            child.unparent();
        }
        if paned.start_child().is_none() {
            paned.set_start_child(Some(&child));
        } else if paned.end_child().is_none() {
            paned.set_end_child(Some(&child));
        } else {
            eprintln!(
                "perry-ui-gtk4: splitview_add_child — Paned only supports 2 children; \
                 third+ child ignored. Use nested SplitViews for N panes."
            );
        }
    }
}
