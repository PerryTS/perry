use std::sync::Once;

use perry_ffi::{gc_register_mutable_root_scanner_named, GcRootVisitor};

static GC_REGISTERED: Once = Once::new();

pub(crate) fn ensure_registered() {
    perry_ui::ensure_gc_scanner_registered();
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ui-macos", scan_roots);
    });
}

fn scan_roots(visitor: &mut GcRootVisitor<'_>) {
    crate::app::scan_macos_app_gc_roots(visitor);
    crate::audio_playback::scan_macos_audio_playback_gc_roots(visitor);
    crate::background::scan_macos_background_gc_roots(visitor);
    crate::deeplinks::scan_macos_deeplinks_gc_roots(visitor);
    crate::drag_drop::scan_macos_drag_drop_gc_roots(visitor);
    crate::geolocation::scan_macos_geolocation_gc_roots(visitor);
    crate::location::scan_macos_location_gc_roots(visitor);
    crate::media_playback::scan_macos_media_playback_gc_roots(visitor);
    crate::menu::scan_macos_menu_gc_roots(visitor);
    crate::network::scan_macos_network_gc_roots(visitor);
    crate::notifications::scan_macos_notifications_gc_roots(visitor);
    crate::pointer::scan_macos_pointer_gc_roots(visitor);
    crate::state::scan_macos_state_gc_roots(visitor);
    crate::tray::scan_macos_tray_gc_roots(visitor);
    crate::widgets::bottom_nav::scan_macos_bottom_nav_gc_roots(visitor);
    crate::widgets::button::scan_macos_button_gc_roots(visitor);
    crate::widgets::calendar::scan_macos_calendar_gc_roots(visitor);
    crate::widgets::combobox::scan_macos_combobox_gc_roots(visitor);
    crate::widgets::command_palette::scan_macos_command_palette_gc_roots(visitor);
    crate::widgets::date_picker::scan_macos_date_picker_gc_roots(visitor);
    crate::widgets::image_gallery::scan_macos_image_gallery_gc_roots(visitor);
    crate::widgets::lazyvstack::scan_macos_lazyvstack_gc_roots(visitor);
    crate::widgets::scan_macos_widgets_gc_roots(visitor);
    crate::widgets::picker::scan_macos_picker_gc_roots(visitor);
    crate::widgets::rich_text::scan_macos_rich_text_gc_roots(visitor);
    crate::widgets::scrollview::scan_macos_scrollview_gc_roots(visitor);
    crate::widgets::securefield::scan_macos_securefield_gc_roots(visitor);
    crate::widgets::slider::scan_macos_slider_gc_roots(visitor);
    crate::widgets::table::scan_macos_table_gc_roots(visitor);
    crate::widgets::textarea::scan_macos_textarea_gc_roots(visitor);
    crate::widgets::textfield::scan_macos_textfield_gc_roots(visitor);
    crate::widgets::toggle::scan_macos_toggle_gc_roots(visitor);
    crate::widgets::toolbar::scan_macos_toolbar_gc_roots(visitor);
    crate::widgets::tree_view::scan_macos_tree_view_gc_roots(visitor);
    crate::widgets::webview::scan_macos_webview_gc_roots(visitor);
}
