// FFI: Layout (width/hugging/insets/match-parent) + TextField extras + app icon.
use crate::widgets;

// =============================================================================
// Layout — width and hugging (GTK4 equivalents of NSLayoutConstraint)
// =============================================================================

/// Set a fixed width constraint on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    widgets::set_width(handle, width);
}

/// Set content hugging priority: high (≥249) → resist hexpand; low → allow hexpand.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(handle: i64, priority: f64) {
    widgets::set_hugging_priority(handle, priority);
}

/// Set edge insets (padding) on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_edge_insets(
    handle: i64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) {
    widgets::set_edge_insets(handle, top, left, bottom, right);
}

/// Get the current text content of a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_get_string(handle: i64) -> i64 {
    widgets::textfield::get_string_value(handle) as i64
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_next_key_view(_handle: i64, _next_handle: i64) {
    // GTK4 handles tab navigation automatically via the widget tree
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_borderless(handle: i64, borderless: f64) {
    widgets::textfield::set_borderless(handle, borderless);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::textfield::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_font_size(handle: i64, size: f64) {
    widgets::textfield::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::textfield::set_text_color(handle, r, g, b, a);
}

/// Make a widget expand to fill its parent's width.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_width(handle: i64) {
    widgets::match_parent_width(handle);
}

/// Make a widget expand to fill its parent's height.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_height(handle: i64) {
    widgets::match_parent_height(handle);
}

/// Set a fixed height constraint on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: i64, height: f64) {
    widgets::set_height(handle, height);
}

/// Set distribution on a stack (GtkBox).
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_distribution(handle: i64, distribution: f64) {
    widgets::set_distribution(handle, distribution as i64);
}

/// Set alignment on a stack (GtkBox).
/// macOS NSLayoutAttribute values: Leading=5, CenterX=9, Width=7, Top=3, CenterY=12, Bottom=4.
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(handle: i64, alignment: f64) {
    widgets::set_alignment(handle, alignment as i64);
}

/// GTK4 already excludes non-visible children from layout — this is a no-op stub.
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_detaches_hidden(handle: i64, flag: i64) {
    widgets::set_detaches_hidden(handle, flag != 0);
}

/// Set the application icon.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(path_ptr: i64) {
    let path = crate::widgets::image::str_from_header(path_ptr as *const u8);
    if path.is_empty() {
        return;
    }

    // Resolve path: try relative to executable, then relative to cwd
    let resolved = resolve_asset_path(path);
    if !resolved.exists() {
        return;
    }

    // In GTK4, window icons are set via the icon theme.
    // Add the icon's parent directory to the theme search path.
    if let Some(display) = gtk4::gdk::Display::default() {
        let theme = gtk4::IconTheme::for_display(&display);
        if let Some(parent) = resolved.parent() {
            theme.add_search_path(parent);
        }
        if let Some(stem) = resolved.file_stem().and_then(|s| s.to_str()) {
            gtk4::Window::set_default_icon_name(stem);
        }
    }
}

/// Resolve an asset path relative to the executable directory.
fn resolve_asset_path(path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() && p.exists() {
        return p.to_path_buf();
    }
    // Try relative to executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(path);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // Fallback to the path as-is (relative to cwd)
    p.to_path_buf()
}

/// Create a VStack with custom insets.
#[no_mangle]
pub extern "C" fn perry_ui_vstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    widgets::vstack::create_with_insets(spacing, top, left, bottom, right)
}

/// Create an HStack with custom insets.
#[no_mangle]
pub extern "C" fn perry_ui_hstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    widgets::hstack::create_with_insets(spacing, top, left, bottom, right)
}
