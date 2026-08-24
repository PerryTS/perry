use std::sync::Once;

static GC_SCANNER_REGISTERED: Once = Once::new();

pub(crate) fn ensure_gc_scanner_registered() {
    perry_ui::ensure_gc_scanner_registered();
    GC_SCANNER_REGISTERED.call_once(|| {
        perry_ffi::gc_register_mutable_root_scanner_named(
            "perry-ui-windows",
            scan_windows_gc_roots,
        );
    });
}

fn scan_windows_gc_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    crate::app::scan_windows_app_gc_roots(visitor);
    crate::drag_drop::scan_windows_drag_drop_gc_roots(visitor);
    crate::media_playback::scan_windows_media_playback_gc_roots(visitor);
    crate::menu::scan_windows_menu_gc_roots(visitor);
    crate::pointer::scan_windows_pointer_gc_roots(visitor);
    crate::state::scan_windows_state_gc_roots(visitor);
    crate::toolbar::scan_windows_toolbar_gc_roots(visitor);
    crate::tray::scan_windows_tray_gc_roots(visitor);
    crate::window::scan_windows_window_gc_roots(visitor);
    crate::widgets::bottom_nav::scan_windows_bottom_nav_gc_roots(visitor);
    crate::widgets::button::scan_windows_button_gc_roots(visitor);
    crate::widgets::calendar::scan_windows_calendar_gc_roots(visitor);
    crate::widgets::combobox::scan_windows_combobox_gc_roots(visitor);
    crate::widgets::command_palette::scan_windows_command_palette_gc_roots(visitor);
    crate::widgets::date_picker::scan_windows_date_picker_gc_roots(visitor);
    crate::widgets::image_gallery::scan_windows_image_gallery_gc_roots(visitor);
    crate::widgets::lazyvstack::scan_windows_lazyvstack_gc_roots(visitor);
    crate::widgets::rich_text::scan_windows_rich_text_gc_roots(visitor);
    crate::widgets::scrollview::scan_windows_scrollview_gc_roots(visitor);
    crate::widgets::slider::scan_windows_slider_gc_roots(visitor);
    crate::widgets::table::scan_windows_table_gc_roots(visitor);
    crate::widgets::tree_view::scan_windows_tree_view_gc_roots(visitor);
    crate::widgets::webview::scan_windows_webview_gc_roots(visitor);
}
