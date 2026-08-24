use std::sync::Once;

static GC_SCANNER_REGISTERED: Once = Once::new();

pub(crate) fn ensure_gc_scanner_registered() {
    perry_ui::ensure_gc_scanner_registered();
    GC_SCANNER_REGISTERED.call_once(|| {
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ui-gtk4", scan_gtk4_gc_roots);
    });
}

fn scan_gtk4_gc_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    crate::app::scan_gtk4_app_gc_roots(visitor);
    crate::audio::scan_gtk4_audio_gc_roots(visitor);
    crate::camera::scan_gtk4_camera_gc_roots(visitor);
    crate::drag_drop::scan_gtk4_drag_drop_gc_roots(visitor);
    crate::ffi::platform_audio_camera_toast::scan_gtk4_platform_ffi_gc_roots(visitor);
    crate::media_playback::scan_gtk4_media_playback_gc_roots(visitor);
    crate::menu::scan_gtk4_menu_gc_roots(visitor);
    crate::state::scan_gtk4_state_gc_roots(visitor);
    crate::toolbar::scan_gtk4_toolbar_gc_roots(visitor);
    crate::tray::scan_gtk4_tray_gc_roots(visitor);
    crate::widgets::bottom_nav::scan_gtk4_bottom_nav_gc_roots(visitor);
    crate::widgets::button::scan_gtk4_button_gc_roots(visitor);
    crate::widgets::command_palette::scan_gtk4_command_palette_gc_roots(visitor);
    crate::widgets::image_gallery::scan_gtk4_image_gallery_gc_roots(visitor);
    crate::widgets::lazyvstack::scan_gtk4_lazyvstack_gc_roots(visitor);
    crate::widgets::picker::scan_gtk4_picker_gc_roots(visitor);
    crate::widgets::scrollview::scan_gtk4_scrollview_gc_roots(visitor);
    crate::widgets::securefield::scan_gtk4_securefield_gc_roots(visitor);
    crate::widgets::slider::scan_gtk4_slider_gc_roots(visitor);
    crate::widgets::textarea::scan_gtk4_textarea_gc_roots(visitor);
    crate::widgets::textfield::scan_gtk4_textfield_gc_roots(visitor);
    crate::widgets::toggle::scan_gtk4_toggle_gc_roots(visitor);
    crate::widgets::webview::scan_gtk4_webview_gc_roots(visitor);
}
