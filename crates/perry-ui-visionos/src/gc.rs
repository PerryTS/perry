use std::sync::Once;

use perry_ffi::{gc_register_mutable_root_scanner_named, GcRootVisitor};

static GC_REGISTERED: Once = Once::new();

pub(crate) fn ensure_registered() {
    perry_ui::ensure_gc_scanner_registered();
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ui-visionos", scan_roots);
    });
}

fn scan_roots(visitor: &mut GcRootVisitor<'_>) {
    crate::app::scan_visionos_app_gc_roots(visitor);
    crate::audio_playback::scan_visionos_audio_playback_gc_roots(visitor);
    crate::background::scan_visionos_background_gc_roots(visitor);
    crate::camera::scan_visionos_camera_gc_roots(visitor);
    crate::drag_drop::scan_visionos_drag_drop_gc_roots(visitor);
    crate::location::scan_visionos_location_gc_roots(visitor);
    crate::media_playback::scan_visionos_media_playback_gc_roots(visitor);
    crate::menu::scan_visionos_menu_gc_roots(visitor);
    crate::pointer::scan_visionos_pointer_gc_roots(visitor);
    crate::state::scan_visionos_state_gc_roots(visitor);
    crate::widgets::bottom_nav::scan_visionos_bottom_nav_gc_roots(visitor);
    crate::widgets::button::scan_visionos_button_gc_roots(visitor);
    crate::widgets::calendar::scan_visionos_calendar_gc_roots(visitor);
    crate::widgets::combobox::scan_visionos_combobox_gc_roots(visitor);
    crate::widgets::date_picker::scan_visionos_date_picker_gc_roots(visitor);
    crate::widgets::scan_visionos_widgets_gc_roots(visitor);
    crate::widgets::rich_text::scan_visionos_rich_text_gc_roots(visitor);
    crate::widgets::scrollview::scan_visionos_scrollview_gc_roots(visitor);
    crate::widgets::securefield::scan_visionos_securefield_gc_roots(visitor);
    crate::widgets::slider::scan_visionos_slider_gc_roots(visitor);
    crate::widgets::tabbar::scan_visionos_tabbar_gc_roots(visitor);
    crate::widgets::textarea::scan_visionos_textarea_gc_roots(visitor);
    crate::widgets::textfield::scan_visionos_textfield_gc_roots(visitor);
    crate::widgets::tree_view::scan_visionos_tree_view_gc_roots(visitor);
    crate::widgets::webview::scan_visionos_webview_gc_roots(visitor);
    crate::widgets::wheel_picker::scan_visionos_wheel_picker_gc_roots(visitor);
}
