use std::sync::Once;

use perry_ffi::{gc_register_mutable_root_scanner_named, GcRootVisitor};

static GC_REGISTERED: Once = Once::new();

pub(crate) fn ensure_registered() {
    perry_ui::ensure_gc_scanner_registered();
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ui-tvos", scan_roots);
    });
}

fn scan_roots(visitor: &mut GcRootVisitor<'_>) {
    crate::app::scan_tvos_app_gc_roots(visitor);
    crate::audio_playback::scan_tvos_audio_playback_gc_roots(visitor);
    crate::background::scan_tvos_background_gc_roots(visitor);
    crate::location::scan_tvos_location_gc_roots(visitor);
    crate::media_playback::scan_tvos_media_playback_gc_roots(visitor);
    crate::menu::scan_tvos_menu_gc_roots(visitor);
    crate::pointer::scan_tvos_pointer_gc_roots(visitor);
    crate::state::scan_tvos_state_gc_roots(visitor);
    crate::widgets::bottom_nav::scan_tvos_bottom_nav_gc_roots(visitor);
    crate::widgets::button::scan_tvos_button_gc_roots(visitor);
    crate::widgets::scan_tvos_widgets_gc_roots(visitor);
    crate::widgets::scrollview::scan_tvos_scrollview_gc_roots(visitor);
    crate::widgets::securefield::scan_tvos_securefield_gc_roots(visitor);
    crate::widgets::slider::scan_tvos_slider_gc_roots(visitor);
    crate::widgets::tabbar::scan_tvos_tabbar_gc_roots(visitor);
    crate::widgets::textarea::scan_tvos_textarea_gc_roots(visitor);
    crate::widgets::textfield::scan_tvos_textfield_gc_roots(visitor);
    crate::widgets::toggle::scan_tvos_toggle_gc_roots(visitor);
}
