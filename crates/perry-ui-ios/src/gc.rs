use std::sync::Once;

use perry_ffi::{gc_register_mutable_root_scanner_named, GcRootVisitor};

static GC_REGISTERED: Once = Once::new();

pub(crate) fn ensure_registered() {
    perry_ui::ensure_gc_scanner_registered();
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ui-ios", scan_roots);
    });
}

fn scan_roots(visitor: &mut GcRootVisitor<'_>) {
    crate::adaptive_layout::scan_ios_adaptive_layout_gc_roots(visitor);
    crate::app::scan_ios_app_gc_roots(visitor);
    crate::audio_playback::scan_ios_audio_playback_gc_roots(visitor);
    crate::background::scan_ios_background_gc_roots(visitor);
    crate::camera::scan_ios_camera_gc_roots(visitor);
    crate::deeplinks::scan_ios_deeplinks_gc_roots(visitor);
    crate::drag_drop::scan_ios_drag_drop_gc_roots(visitor);
    crate::geolocation::scan_ios_geolocation_gc_roots(visitor);
    crate::location::scan_ios_location_gc_roots(visitor);
    crate::media_playback::scan_ios_media_playback_gc_roots(visitor);
    crate::menu::scan_ios_menu_gc_roots(visitor);
    crate::network::scan_ios_network_gc_roots(visitor);
    crate::notifications::scan_ios_notifications_gc_roots(visitor);
    crate::pointer::scan_ios_pointer_gc_roots(visitor);
    crate::state::scan_ios_state_gc_roots(visitor);
    crate::widgets::bottom_nav::scan_ios_bottom_nav_gc_roots(visitor);
    crate::widgets::button::scan_ios_button_gc_roots(visitor);
    crate::widgets::calendar::scan_ios_calendar_gc_roots(visitor);
    crate::widgets::combobox::scan_ios_combobox_gc_roots(visitor);
    crate::widgets::date_picker::scan_ios_date_picker_gc_roots(visitor);
    crate::widgets::image_gallery::scan_ios_image_gallery_gc_roots(visitor);
    crate::widgets::scan_ios_widgets_gc_roots(visitor);
    crate::widgets::picker::scan_ios_picker_gc_roots(visitor);
    crate::widgets::rich_text::scan_ios_rich_text_gc_roots(visitor);
    crate::widgets::scrollview::scan_ios_scrollview_gc_roots(visitor);
    crate::widgets::securefield::scan_ios_securefield_gc_roots(visitor);
    crate::widgets::slider::scan_ios_slider_gc_roots(visitor);
    crate::widgets::tabbar::scan_ios_tabbar_gc_roots(visitor);
    crate::widgets::textarea::scan_ios_textarea_gc_roots(visitor);
    crate::widgets::textfield::scan_ios_textfield_gc_roots(visitor);
    crate::widgets::toggle::scan_ios_toggle_gc_roots(visitor);
    crate::widgets::tree_view::scan_ios_tree_view_gc_roots(visitor);
    crate::widgets::webview::scan_ios_webview_gc_roots(visitor);
    crate::widgets::wheel_picker::scan_ios_wheel_picker_gc_roots(visitor);
}
