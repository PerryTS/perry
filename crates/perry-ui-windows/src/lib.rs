pub mod app;
pub mod audio;
pub mod deeplinks_stub;
#[cfg(target_os = "windows")]
pub mod dpi_compat;
pub mod issue_552_stub;
pub mod media_playback;
pub mod network_stub;

// Install a vectored exception handler that prints crash info to stderr.
#[cfg(target_os = "windows")]
mod crash_handler {
    #[repr(C)]
    struct ExceptionRecord {
        exception_code: u32,
        exception_flags: u32,
        exception_record: *mut ExceptionRecord,
        exception_address: *mut core::ffi::c_void,
        number_parameters: u32,
        exception_information: [usize; 15],
    }

    // Accurate x64 Windows CONTEXT layout for the two fields we need.
    // (The previous `_padding:[u8;0x78]; Rip` mislabeled offset 0x78 — that
    // is Rax on x64; Rip is at 0xF8 — but nothing read it, so the bug was
    // dormant. We need Rsp@0x98 to recover the call chain.)
    #[repr(C)]
    #[allow(non_snake_case)]
    struct Context {
        _pad0: [u8; 0x98],              // → Rsp at 0x98
        Rsp: u64,                       // 0x98
        _pad1: [u8; 0xF8 - (0x98 + 8)], // 0xA0..0xF8
        Rip: u64,                       // 0xF8
    }

    #[repr(C)]
    struct ExceptionPointers {
        exception_record: *mut ExceptionRecord,
        context_record: *mut Context,
    }

    extern "system" {
        fn AddVectoredExceptionHandler(
            first: u32,
            handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32,
        ) -> *mut core::ffi::c_void;
        fn GetModuleHandleW(name: *const u16) -> *mut core::ffi::c_void;
    }

    use std::sync::atomic::{AtomicBool, Ordering};
    // The rich dump itself reads raw stack memory; if that ever faults we
    // must not re-enter and loop. One dump is all a diagnostic needs.
    static DUMPED: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn handler(info: *mut ExceptionPointers) -> i32 {
        let info = &*info;
        let record = &*info.exception_record;
        // 0xC0000005 = ACCESS_VIOLATION
        if record.exception_code == 0xC0000005 {
            let addr = if record.number_parameters >= 2 {
                record.exception_information[1]
            } else {
                0
            };
            let rip = record.exception_address as usize;
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr(),
                "[CRASH] ACCESS_VIOLATION at code=0x{:X} accessing 0x{:X}",
                rip,
                addr
            );
            // Recover the faulting call chain. A RIP of 0 / wild address is
            // a call through a null/garbage function pointer — the pushed
            // return address at [Rsp] points straight at the culprit. We
            // can't symbolize in-process safely from a VEH (no DbgHelp), so
            // emit the module base + raw addresses and their module-relative
            // RVAs; with `--debug-symbols` (#896) producing a PDB these
            // resolve offline via `llvm-symbolizer --obj=<exe> <rva>`.
            if !DUMPED.swap(true, Ordering::SeqCst) && !info.context_record.is_null() {
                let ctx = &*info.context_record;
                let base = GetModuleHandleW(core::ptr::null()) as usize;
                // 256 MiB code window — generous; perry binaries are tens of MiB.
                let win = 0x1000_0000usize;
                let in_mod = |a: usize| base != 0 && a >= base && a < base + win;
                let _ = writeln!(
                    std::io::stderr(),
                    "[CRASH] rip=0x{:X} rsp=0x{:X} module_base=0x{:X}{}",
                    ctx.Rip as usize,
                    ctx.Rsp as usize,
                    base,
                    if in_mod(ctx.Rip as usize) {
                        format!(" rip_rva=+0x{:X}", ctx.Rip as usize - base)
                    } else {
                        String::new()
                    }
                );
                // Scan the top of the faulting stack for return-address-shaped
                // values (anything inside the main module's code window). The
                // first is the immediate caller of the bad call; the rest
                // approximate the chain above it.
                let sp = ctx.Rsp as *const usize;
                let mut printed = 0;
                let mut i = 0usize;
                while i < 512 && printed < 24 {
                    let v = *sp.add(i);
                    if in_mod(v) {
                        let _ = writeln!(
                            std::io::stderr(),
                            "[CRASH] stack[+0x{:X}] = 0x{:X}  rva=+0x{:X}",
                            i * 8,
                            v,
                            v - base
                        );
                        printed += 1;
                    }
                    i += 1;
                }
            }
            let _ = std::io::stderr().flush();
        }
        0 // EXCEPTION_CONTINUE_SEARCH
    }

    #[used]
    #[link_section = ".CRT$XCU"]
    static INSTALL_HANDLER: unsafe extern "C" fn() = {
        unsafe extern "C" fn install() {
            AddVectoredExceptionHandler(1, handler);
        }
        install
    };
}
pub mod clipboard;
pub mod dialog;
pub mod file_dialog;
pub mod folder_dialog;
pub mod keychain;
pub mod layout;
pub mod menu;
pub mod sheet;
pub mod state;
pub mod system;
pub mod toolbar;
pub mod tray;
pub mod widgets;
pub mod window;

pub mod screenshot;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

// =============================================================================
// FFI exports — these are the functions called from codegen-generated code
// =============================================================================

/// Create an app. title_ptr=raw string, width/height as f64.
/// Returns app handle (i64).
#[no_mangle]
pub extern "C" fn perry_ui_app_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    app::app_create(title_ptr as *const u8, width, height)
}

/// Set the root widget of an app.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_body(app_handle: i64, root_handle: i64) {
    app::app_set_body(app_handle, root_handle);
}

/// Run the app event loop (blocks until window closes).
#[no_mangle]
pub extern "C" fn perry_ui_app_run(app_handle: i64) {
    app::app_run(app_handle);
}

/// Resize the main app window.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_size(app_handle: i64, width: f64, height: f64) {
    app::app_set_size(app_handle, width, height);
}

/// Set minimum window size.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_min_size(app_handle: i64, w: f64, h: f64) {
    app::set_min_size(app_handle, w, h);
}

/// Set maximum window size.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_max_size(app_handle: i64, w: f64, h: f64) {
    app::set_max_size(app_handle, w, h);
}

/// Register callback for app activation.
#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(callback: f64) {
    app::on_activate(callback);
}

/// Register callback for app termination.
#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(callback: f64) {
    app::on_terminate(callback);
}

/// Set a repeating timer.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_timer(_app_handle: i64, interval_ms: f64, callback: f64) {
    // Dispatch passes the App handle as the first arg (matching TS surface
    // `appSetTimer(app, intervalMs, callback)`). Without consuming it here
    // the f64 args land in the wrong XMM slots on Win64 ABI.
    app::set_timer(interval_ms, callback);
}

// =============================================================================
// Multi-Window
// =============================================================================

/// Create a new window.
#[no_mangle]
pub extern "C" fn perry_ui_window_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    window::create(title_ptr as *const u8, width, height)
}

/// Set the body of a window.
#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(window_handle: i64, widget_handle: i64) {
    window::set_body(window_handle, widget_handle);
}

/// Show a window.
#[no_mangle]
pub extern "C" fn perry_ui_window_show(window_handle: i64) {
    window::show(window_handle);
}

/// Close a window.
#[no_mangle]
pub extern "C" fn perry_ui_window_close(window_handle: i64) {
    window::close(window_handle);
}

/// Hide a window without destroying it.
#[no_mangle]
pub extern "C" fn perry_ui_window_hide(window_handle: i64) {
    window::hide(window_handle);
}

/// Set window size.
#[no_mangle]
pub extern "C" fn perry_ui_window_set_size(window_handle: i64, width: f64, height: f64) {
    window::set_size(window_handle, width, height);
}

/// Register a callback for when the window loses focus.
#[no_mangle]
pub extern "C" fn perry_ui_window_on_focus_lost(window_handle: i64, callback: f64) {
    window::on_focus_lost(window_handle, callback);
}

// =============================================================================
// Widget Creation
// =============================================================================

/// Create a Text label.
#[no_mangle]
pub extern "C" fn perry_ui_text_create(text_ptr: i64) -> i64 {
    widgets::text::create(text_ptr as *const u8)
}

/// Create a Button.
#[no_mangle]
pub extern "C" fn perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64 {
    widgets::button::create(label_ptr as *const u8, on_press)
}

/// Create a VStack container (spacing DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_vstack_create(spacing: f64) -> i64 {
    widgets::vstack::create(spacing * app::get_dpi_scale())
}

/// Create an HStack container (spacing DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_hstack_create(spacing: f64) -> i64 {
    widgets::hstack::create(spacing * app::get_dpi_scale())
}

/// Create a Spacer.
#[no_mangle]
pub extern "C" fn perry_ui_spacer_create() -> i64 {
    widgets::spacer::create()
}

/// Create a Divider.
#[no_mangle]
pub extern "C" fn perry_ui_divider_create() -> i64 {
    widgets::divider::create()
}

/// Create a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textfield::create(placeholder_ptr as *const u8, on_change)
}

/// Create a SecureField (password entry).
#[no_mangle]
pub extern "C" fn perry_ui_securefield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::securefield::create(placeholder_ptr as *const u8, on_change)
}

/// Create a Toggle.
#[no_mangle]
pub extern "C" fn perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64 {
    widgets::toggle::create(label_ptr as *const u8, on_change)
}

/// Create a Slider.
#[no_mangle]
pub extern "C" fn perry_ui_slider_create(min: f64, max: f64, on_change: f64) -> i64 {
    // Codegen emits 3-arg `Slider(min, max, onChange)`; default initial=min.
    widgets::slider::create(min, max, min, on_change)
}

/// Create a ScrollView.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    widgets::scrollview::create()
}

/// Create a Canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_create(width: f64, height: f64) -> i64 {
    widgets::canvas::create(width, height)
}

/// Create a Form container.
#[no_mangle]
pub extern "C" fn perry_ui_form_create() -> i64 {
    widgets::form::create()
}

/// Create a Section with title.
#[no_mangle]
pub extern "C" fn perry_ui_section_create(title_ptr: i64) -> i64 {
    widgets::form::section_create(title_ptr as *const u8)
}

/// Create a ZStack (overlay container).
#[no_mangle]
pub extern "C" fn perry_ui_zstack_create() -> i64 {
    widgets::zstack::create()
}

/// Create a LazyVStack.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(count: f64, render_closure: f64) -> i64 {
    widgets::lazyvstack::create(count, render_closure)
}

/// Update a LazyVStack with a new item count.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(handle: i64, count: i64) {
    widgets::lazyvstack::update(handle, count);
}

/// Advisory on Windows — eager-render path doesn't consult row_height yet.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_row_height(_handle: i64, _height: f64) {}

// Table (stub — not yet implemented on Windows)
#[no_mangle]
pub extern "C" fn perry_ui_table_create(row_count: f64, col_count: f64, render: f64) -> i64 {
    widgets::table::create(row_count, col_count, render)
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(handle: i64, col: i64, title_ptr: i64) {
    widgets::table::set_column_header(handle, col, title_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(handle: i64, col: i64, width: f64) {
    widgets::table::set_column_width(handle, col, width);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(handle: i64, count: i64) {
    widgets::table::update_row_count(handle, count);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(handle: i64, callback: f64) {
    widgets::table::set_on_row_select(handle, callback);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(handle: i64) -> i64 {
    widgets::table::get_selected_row(handle)
}

/// Create a ProgressView.
#[no_mangle]
pub extern "C" fn perry_ui_progressview_create() -> i64 {
    widgets::progressview::create()
}

/// Set progress value (0.0-1.0, negative = indeterminate).
#[no_mangle]
pub extern "C" fn perry_ui_progressview_set_value(handle: i64, value: f64) {
    widgets::progressview::set_value(handle, value);
}

// =============================================================================
// Child Management
// =============================================================================

/// Add a child widget to a parent.
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child(parent_handle: i64, child_handle: i64) {
    widgets::add_child(parent_handle, child_handle);
    app::request_layout();
}

/// Remove a child widget from a parent.
#[no_mangle]
pub extern "C" fn perry_ui_widget_remove_child(parent_handle: i64, child_handle: i64) {
    widgets::remove_child(parent_handle, child_handle);
    app::request_layout();
}

// =============================================================================
// State System
// =============================================================================

/// Create a reactive state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_create(initial: f64) -> i64 {
    state::state_create(initial)
}

/// Get the current value of a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_get(state_handle: i64) -> f64 {
    state::state_get(state_handle)
}

/// Set a new value on a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_set(state_handle: i64, value: f64) {
    state::state_set(state_handle, value);
}

/// Register an onChange callback for a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_on_change(state_handle: i64, callback: f64) {
    state::on_change(state_handle, callback);
}

// =============================================================================
// State Bindings
// =============================================================================

/// Bind a text widget to a state cell with prefix/suffix.
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_text_numeric(
    state_handle: i64,
    text_handle: i64,
    prefix_ptr: i64,
    suffix_ptr: i64,
) {
    state::bind_text_numeric(
        state_handle,
        text_handle,
        prefix_ptr as *const u8,
        suffix_ptr as *const u8,
    );
}

/// Bind a slider to a state cell (two-way).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_slider(state_handle: i64, slider_handle: i64) {
    state::bind_slider(state_handle, slider_handle);
}

/// Bind a toggle to a state cell (two-way).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_toggle(state_handle: i64, toggle_handle: i64) {
    state::bind_toggle(state_handle, toggle_handle);
}

/// Bind a text widget to multiple states with a template.
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_text_template(
    text_handle: i64,
    num_parts: i32,
    types_ptr: i64,
    values_ptr: i64,
) {
    state::bind_text_template(
        text_handle,
        num_parts,
        types_ptr as *const i32,
        values_ptr as *const i64,
    );
}

/// Bind visibility of widgets to a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_visibility(
    state_handle: i64,
    show_handle: i64,
    hide_handle: i64,
) {
    state::bind_visibility(state_handle, show_handle, hide_handle);
}

/// Bind a textfield to a state cell (two-way).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

/// Initialize a ForEach dynamic list binding.
#[no_mangle]
pub extern "C" fn perry_ui_for_each_init(
    container_handle: i64,
    state_handle: i64,
    render_closure: f64,
) {
    state::for_each_init(container_handle, state_handle, render_closure);
}

/// Remove all children from a container widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_clear_children(handle: i64) {
    widgets::clear_children(handle);
    app::request_layout();
}

// =============================================================================
// Text Styling
// =============================================================================

/// Set the text content of a Text widget.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_string(handle: i64, text_ptr: i64) {
    widgets::text::set_string(handle, text_ptr as *const u8);
}

/// Set the text color.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::text::set_color(handle, r, g, b, a);
}

/// Set the font size.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_size(handle: i64, size: f64) {
    widgets::text::set_font_size(handle, size);
}

/// Set the font weight.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_weight(handle: i64, size: f64, weight: f64) {
    widgets::text::set_font_weight(handle, size, weight);
}

/// Set whether text is selectable.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(handle: i64, selectable: f64) {
    widgets::text::set_selectable(handle, selectable != 0.0);
}

/// Set text decoration (issue #185 Phase B closure). Currently
/// stub-with-state on Windows; see `widgets::text::set_decoration`
/// for rationale.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_decoration(handle: i64, decoration: i64) {
    widgets::text::set_decoration(handle, decoration);
}

/// Issue #707 — cap visible lines on a Win32 STATIC control via the
/// SS_*ELLIPSIS style bits.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_number_of_lines(handle: i64, lines: i64) {
    widgets::text::set_number_of_lines(handle, lines);
}
/// Issue #707 — STATIC truncation mode.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_truncation_mode(handle: i64, mode: i64) {
    widgets::text::set_truncation_mode(handle, mode);
}

/// Set the font family.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_family(handle: i64, family_ptr: i64) {
    widgets::text::set_font_family(handle, family_ptr as *const u8);
}

// =============================================================================
// Button Ops
// =============================================================================

/// Set whether a button has a border.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_bordered(handle: i64, bordered: f64) {
    widgets::button::set_bordered(handle, bordered != 0.0);
}

/// Set the title of a button.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_title(handle: i64, title_ptr: i64) {
    widgets::button::set_title(handle, title_ptr as *const u8);
}

/// Set button image (SF Symbol name). On Windows, maps known SF Symbol names to Unicode/text fallbacks.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_image(handle: i64, name_ptr: i64) {
    widgets::button::set_image(handle, name_ptr as *const u8);
}

/// Set button image position. No-op on Windows (our "images" are text).
#[no_mangle]
pub extern "C" fn perry_ui_button_set_image_position(_handle: i64, _position: f64) {}

/// Set button content tint color. On Windows, delegates to text color since icons are text.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_content_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::button::set_text_color(handle, r, g, b, a);
}

// =============================================================================
// TextField Ops
// =============================================================================

/// Focus a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(handle: i64) {
    widgets::textfield::focus(handle);
}

/// Set the text value of a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_string(handle: i64, text_ptr: i64) {
    widgets::textfield::set_string_value(handle, text_ptr as *const u8);
}

// =============================================================================
// ScrollView
// =============================================================================

/// Set the child of a ScrollView.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::set_child(scroll_handle, child_handle);
}

/// Scroll to make a child visible.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::scroll_to(scroll_handle, child_handle);
}

/// Get scroll offset.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64 {
    widgets::scrollview::get_offset(scroll_handle)
}

/// Set scroll offset.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64) {
    widgets::scrollview::set_offset(scroll_handle, offset);
}

// =============================================================================
// Styling
// =============================================================================

/// Set background color.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::set_background_color(handle, r, g, b, a);
}

/// Set background gradient.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    widgets::set_background_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

/// Set corner radius.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_corner_radius(handle: i64, radius: f64) {
    widgets::set_corner_radius(handle, radius);
}

/// Set drop shadow on a widget (issue #185 Phase B / #210 closure).
///
/// Wired via a parent-window WM_PAINT subclass that renders the shadow
/// onto the parent's surface using `AlphaBlend` against a 32bpp DIB
/// section. Per-pixel falloff is a quadratic Gaussian approximation —
/// see `widgets::paint_shadow_for_child` for the rendering math.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_shadow(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
) {
    widgets::set_shadow(handle, r, g, b, a, blur, offset_x, offset_y);
}

/// Set static opacity on a widget (issue #185 Phase B closure).
/// Currently stub-with-state; see `widgets::set_opacity` for rationale.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_opacity(handle: i64, opacity: f64) {
    widgets::set_opacity(handle, opacity);
}

/// Set border color (issue #185 Phase B closure). Stub-with-state.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::set_border_color(handle, r, g, b, a);
}

/// Set border width (issue #185 Phase B closure). Stub-with-state.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_width(handle: i64, width: f64) {
    widgets::set_border_width(handle, width);
}

/// Set context menu on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_context_menu(widget_handle: i64, menu_handle: i64) {
    menu::set_context_menu(widget_handle, menu_handle);
}

/// Set control size.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_control_size(handle: i64, size: i64) {
    widgets::set_control_size(handle, size);
}

/// Set enabled/disabled.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_enabled(handle: i64, enabled: i64) {
    widgets::set_enabled(handle, enabled != 0);
}

/// Set tooltip.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    widgets::set_tooltip(handle, text_ptr as *const u8);
}

/// Rich tooltip — popup HWND hosting an arbitrary widget tree, shown
/// after the configured hover delay. Win32 ToolTip class is text-only,
/// so we roll our own popup that re-parents the content widget on show
/// and detaches it on hide. See `widgets::rich_tooltip` (#479 / #11).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_rich_tooltip(
    handle: i64,
    content_handle: i64,
    hover_delay_ms: f64,
) {
    widgets::rich_tooltip::set_rich_tooltip(handle, content_handle, hover_delay_ms);
}

/// Set hidden state. Triggers a layout pass so newly visible widgets get sized.
#[no_mangle]
pub extern "C" fn perry_ui_set_widget_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
    app::request_layout();
}

// =============================================================================
// Canvas
// =============================================================================

/// Clear a canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear(handle: i64) {
    widgets::canvas::clear(handle);
}

/// Begin a new path on a canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_begin_path(handle: i64) {
    widgets::canvas::begin_path(handle);
}

/// Move the path cursor.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_move_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::move_to(handle, x, y);
}

/// Draw a line to a point.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_line_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::line_to(handle, x, y);
}

/// Stroke the current path.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    line_width: f64,
) {
    widgets::canvas::stroke(handle, r, g, b, a, line_width);
}

/// Fill the current path with a gradient.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    widgets::canvas::fill_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_fill_color(_h: i64, _r: f64, _g: f64, _b: f64, _a: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_stroke_color(_h: i64, _r: f64, _g: f64, _b: f64, _a: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_line_width(_h: i64, _w: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_rect(_h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_rect(_h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear_rect(h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {
    widgets::canvas::clear(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_arc(_h: i64, _x: f64, _y: f64, _r: f64, _sa: f64, _ea: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_close_path(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_path(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_text(_h: i64, _ptr: i64, _x: f64, _y: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_font(_h: i64, _ptr: i64) {}

// =============================================================================
// Menu
// =============================================================================

/// Create a context menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_create() -> i64 {
    menu::create()
}

/// Add an item to a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item(menu_handle: i64, title_ptr: i64, callback: f64) {
    menu::add_item(menu_handle, title_ptr as *const u8, callback);
}

/// Add a menu item with a keyboard shortcut.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item_with_shortcut(
    menu_handle: i64,
    title_ptr: i64,
    shortcut_ptr: i64,
    callback: f64,
) {
    // Arg order matches the TS-side API: `menuAddItemWithShortcut(menu, title, shortcut, callback)`.
    // Why: on Win64 ABI int and float positional slots share register indices —
    // `(i64, i64, f64, i64)` vs caller's `(i64, i64, i64, f64)` would put `callback`
    // in XMM2 (uninitialized) and `shortcut_ptr` in R9 (also uninitialized), causing
    // a deref-garbage ACCESS_VIOLATION inside `str_from_header(shortcut_ptr)`.
    menu::add_item_with_shortcut(
        menu_handle,
        title_ptr as *const u8,
        callback,
        shortcut_ptr as *const u8,
    );
}

/// Add a separator to a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_separator(menu_handle: i64) {
    menu::add_separator(menu_handle);
}

/// Add a submenu to a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_submenu(menu_handle: i64, title_ptr: i64, submenu_handle: i64) {
    menu::add_submenu(menu_handle, title_ptr as *const u8, submenu_handle);
}

/// Create a menu bar. Returns bar handle.
#[no_mangle]
pub extern "C" fn perry_ui_menubar_create() -> i64 {
    menu::menubar_create()
}

/// Add a menu to a menu bar with a title.
#[no_mangle]
pub extern "C" fn perry_ui_menubar_add_menu(bar_handle: i64, title_ptr: i64, menu_handle: i64) {
    menu::menubar_add_menu(bar_handle, title_ptr as *const u8, menu_handle);
}

/// Attach a menu bar to the application.
#[no_mangle]
pub extern "C" fn perry_ui_menubar_attach(bar_handle: i64) {
    menu::menubar_attach(bar_handle);
}

// =============================================================================
// Tray icon (issue #490)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_tray_create(icon_path_ptr: i64) -> i64 {
    tray::create(icon_path_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_icon(tray_handle: i64, icon_path_ptr: i64) {
    tray::set_icon(tray_handle, icon_path_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_tooltip(tray_handle: i64, tooltip_ptr: i64) {
    tray::set_tooltip(tray_handle, tooltip_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_attach_menu(tray_handle: i64, menu_handle: i64) {
    tray::attach_menu(tray_handle, menu_handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_on_click(tray_handle: i64, callback: f64) {
    tray::on_click(tray_handle, callback);
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_destroy(tray_handle: i64) {
    tray::destroy(tray_handle);
}

/// Remove all items from a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(menu_handle: i64) {
    menu::clear(menu_handle);
}

/// Add a menu item with a standard action (no-op on Windows — macOS responder chain concept).
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(
    _menu_handle: i64,
    _title_ptr: i64,
    _selector_ptr: i64,
    _shortcut_ptr: i64,
) {
    // No-op on Windows — standard actions (copy/paste/undo) are handled by
    // the system via WM_COMMAND and accelerator tables, not ObjC selectors.
}

// =============================================================================
// Clipboard
// =============================================================================

/// Read from clipboard.
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    clipboard::read()
}

/// Write to clipboard.
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(text_ptr: i64) {
    clipboard::write(text_ptr as *const u8);
}

// =============================================================================
// Dialog
// =============================================================================

/// Open a file dialog.
#[no_mangle]
pub extern "C" fn perry_ui_open_file_dialog(callback: f64) {
    file_dialog::open_dialog(callback);
}

/// Open a folder dialog.
#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    folder_dialog::open_dialog(callback);
}

/// Open a save file dialog.
#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    callback: f64,
    default_name_ptr: i64,
    allowed_types_ptr: i64,
) {
    dialog::save_file_dialog(
        callback,
        default_name_ptr as *const u8,
        allowed_types_ptr as *const u8,
    );
}

/// Show an alert dialog with custom buttons.
/// `buttons` is a NaN-boxed JS array of string labels; callback receives index.
#[no_mangle]
pub extern "C" fn perry_ui_alert(title_ptr: i64, message_ptr: i64, buttons: f64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let buttons_ptr = unsafe { js_nanbox_get_pointer(buttons) } as *const u8;
    dialog::alert(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr,
        callback,
    );
}

/// Show a simple alert (title, message, OK button). Called from `alert(title, message)`.
#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(title_ptr: i64, message_ptr: i64) {
    dialog::alert_simple(title_ptr as *const u8, message_ptr as *const u8);
}

// =============================================================================
// Keyboard Shortcut
// =============================================================================

/// Add a keyboard shortcut.
#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64) {
    app::add_keyboard_shortcut(key_ptr as *const u8, modifiers, callback);
}

/// Register a system-wide global hotkey (Win32 RegisterHotKey).
#[no_mangle]
pub extern "C" fn perry_ui_register_global_hotkey(key_ptr: i64, modifiers: f64, callback: f64) {
    app::register_global_hotkey(key_ptr as *const u8, modifiers, callback);
}

/// Get the icon for an application at the given path. Returns a widget handle or 0.
#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(path_ptr: i64) -> i64 {
    app::get_app_icon(path_ptr as *const u8)
}

// =============================================================================
// Events
// =============================================================================

/// Set an on-hover callback.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_hover(handle: i64, callback: f64) {
    widgets::set_on_hover(handle, callback);
}

/// Set a double-click callback.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_double_click(handle: i64, callback: f64) {
    widgets::set_on_double_click(handle, callback);
}

// =============================================================================
// Animation
// =============================================================================

/// Animate opacity. `duration_secs` is in seconds.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    widgets::animate_opacity(handle, target, duration_secs);
}

/// Animate position. `duration_secs` is in seconds.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_position(
    handle: i64,
    dx: f64,
    dy: f64,
    duration_secs: f64,
) {
    widgets::animate_position(handle, dx, dy, duration_secs);
}

// =============================================================================
// Layout
// =============================================================================

/// Create a VStack with custom insets (DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_vstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    let s = app::get_dpi_scale();
    widgets::vstack::create_with_insets(spacing * s, top * s, left * s, bottom * s, right * s)
}

/// Create an HStack with custom insets (DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_hstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    let s = app::get_dpi_scale();
    widgets::hstack::create_with_insets(spacing * s, top * s, left * s, bottom * s, right * s)
}

// =============================================================================
// Navigation
// =============================================================================

/// Create a NavigationStack with initial page.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_create() -> i64 {
    // Dispatch (perry-dispatch::PERRY_UI_TABLE) emits this call with 0 args
    // because the TS-side API is `NavStack(): Widget`. The previous 2-arg
    // signature read uninitialized RCX/RDX on Win64 — `str_from_header(garbage)`
    // dereffed wild memory and crashed with ACCESS_VIOLATION. SysV (macOS/Linux)
    // happened to land 0 in those registers most of the time, masking the bug.
    widgets::navstack::create(std::ptr::null(), 0)
}

/// Push a page onto the navigation stack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_push(handle: i64, title_ptr: i64, body_handle: i64) {
    widgets::navstack::push(handle, title_ptr as *const u8, body_handle);
}

/// Pop the top page from the navigation stack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_pop(handle: i64) {
    widgets::navstack::pop(handle);
}

// =============================================================================
// Picker
// =============================================================================

/// Create a Picker (dropdown).
#[no_mangle]
// Issue #478 — Rich text editor — real Windows impl via RichEdit (MSFTEDIT_CLASS).
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_create(w: f64, h: f64, cb: f64) -> i64 {
    widgets::rich_text::create(w, h, cb)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_string(h: i64, t: i64) {
    widgets::rich_text::set_string(h, t as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_string(h: i64) -> f64 {
    widgets::rich_text::get_string(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_html(h: i64, html: i64) -> i64 {
    widgets::rich_text::set_html(h, html as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_html(h: i64) -> f64 {
    widgets::rich_text::get_html(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_bold(h: i64) {
    widgets::rich_text::toggle_bold(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_italic(h: i64) {
    widgets::rich_text::toggle_italic(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_underline(h: i64) {
    widgets::rich_text::toggle_underline(h)
}

// PdfView (#516) — Win32 stub-with-state. STATIC label shows
// "[PDF: name — page X/Y @ Z%]" on load + nav. Real page-bitmap
// rendering via `Windows.Data.Pdf` WinRT or PDFium is a follow-up.
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_create(w: f64, h: f64) -> i64 {
    widgets::pdf_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_load_file(h: i64, p: i64) -> i64 {
    widgets::pdf_view::load_file(h, p as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_page_count(h: i64) -> i64 {
    widgets::pdf_view::get_page_count(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_go_to_page(h: i64, i: i64) {
    widgets::pdf_view::go_to_page(h, i)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_current_page(h: i64) -> i64 {
    widgets::pdf_view::get_current_page(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_set_scale(h: i64, s: f64) {
    widgets::pdf_view::set_scale(h, s)
}

// MapView (#517 / #559) — Win32 stub-with-state. STATIC label shows
// the current region + pin count. Real WinUI MapControl in XAML
// Islands needs Windows App SDK + WinUI 3 stack + Bing Maps API key
// — tracked under #559 as multi-day follow-up.
#[no_mangle]
pub extern "C" fn perry_ui_map_view_create(w: f64, h: f64) -> i64 {
    widgets::map_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_region(h: i64, lat: f64, lon: f64, ls: f64, os: f64) {
    widgets::map_view::set_region(h, lat, lon, ls, os);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_add_pin(h: i64, lat: f64, lon: f64, t: i64) {
    widgets::map_view::add_pin(h, lat, lon, t as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_clear_pins(h: i64) {
    widgets::map_view::clear_pins(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_map_type(h: i64, s: i64) {
    widgets::map_view::set_map_type(h, s);
}

// Issue #477 — Command palette stubs.
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_register(
    id: i64,
    label: i64,
    subtitle: i64,
    on_run: f64,
) {
    widgets::command_palette::register(
        id as *const u8,
        label as *const u8,
        subtitle as *const u8,
        on_run,
    );
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_unregister(id: i64) {
    widgets::command_palette::unregister(id as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_clear() {
    widgets::command_palette::clear();
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_show() {
    widgets::command_palette::show();
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_hide() {
    widgets::command_palette::hide();
}

// Issue #474 — Chart widget — real Windows impl via GDI on owner-draw HWND.
#[no_mangle]
pub extern "C" fn perry_ui_chart_create(kind: i64, w: f64, h: f64) -> i64 {
    widgets::chart::create(kind, w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_add_data_point(h: i64, l: i64, v: f64) {
    widgets::chart::add_data_point(h, l as *const u8, v)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_clear_data(h: i64) {
    widgets::chart::clear_data(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_set_title(h: i64, t: i64) {
    widgets::chart::set_title(h, t as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_reload(h: i64) {
    widgets::chart::reload(h)
}

// Issue #481 — Calendar widget — real Windows impl via SysMonthCal32.
#[no_mangle]
pub extern "C" fn perry_ui_calendar_create(year: i64, month: i64, on_change: f64) -> i64 {
    widgets::calendar::create(year, month, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_set_date(h: i64, y: i64, m: i64, d: i64) {
    widgets::calendar::set_date(h, y, m, d)
}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_get_selected_date(h: i64) -> f64 {
    widgets::calendar::get_selected_date(h)
}

// Issue #473 — table sort / filter / multi-select. Real LVS_REPORT impl
// lives in `widgets::table`; the bare ABI shape mirrors macOS so dispatch
// signatures match across platforms (#7 — ListView with column headers).
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_sort_change(h: i64, cb: f64) {
    widgets::table::set_on_sort_change(h, cb);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_allows_multiple_selection(h: i64, allow: i64) {
    widgets::table::set_allows_multiple_selection(h, allow);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_rows_count(h: i64) -> i64 {
    widgets::table::get_selected_rows_count(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row_at(h: i64, n: i64) -> i64 {
    widgets::table::get_selected_row_at(h, n)
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_filter_text(h: i64, t: i64) {
    widgets::table::set_filter_text(h, t as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_filter_text(h: i64) -> f64 {
    let ptr = widgets::table::get_filter_text(h);
    // Wrap as NaN-boxed STRING_TAG (top16 = 0x7FFF, lower 48 = pointer).
    f64::from_bits(0x7FFF_0000_0000_0000_u64 | (ptr as u64 & 0x0000_FFFF_FFFF_FFFF))
}

/// TreeView (#480) — real Win32 impl via SysTreeView32 + TVN_SELCHANGEDW.
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_create(id_ptr: i64, label_ptr: i64) -> i64 {
    widgets::tree_view::node_create(id_ptr as *const u8, label_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_add_child(parent: i64, child: i64) {
    widgets::tree_view::node_add_child(parent, child)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_create(root: i64, on_select: f64) -> i64 {
    widgets::tree_view::create(root, on_select)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_expand_all(handle: i64) {
    widgets::tree_view::expand_all(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_collapse_all(handle: i64) {
    widgets::tree_view::collapse_all(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_get_selected_id(handle: i64) -> f64 {
    widgets::tree_view::get_selected_id(handle)
}

/// Combobox (#475) — real Win32 impl via CBS_DROPDOWN ComboBox.
#[no_mangle]
pub extern "C" fn perry_ui_combobox_create(initial_ptr: i64, on_change: f64) -> i64 {
    widgets::combobox::create(initial_ptr as *const u8, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_add_item(handle: i64, value_ptr: i64) {
    widgets::combobox::add_item(handle, value_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_set_value(handle: i64, value_ptr: i64) {
    widgets::combobox::set_value(handle, value_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_get_value(handle: i64) -> f64 {
    widgets::combobox::get_value(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_create(label_ptr: i64, on_change: f64, style: i64) -> i64 {
    widgets::picker::create(label_ptr as *const u8, on_change, style)
}

/// Add an item to a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_add_item(handle: i64, title_ptr: i64) {
    widgets::picker::add_item(handle, title_ptr as *const u8);
}

/// Set the selected item of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_set_selected(handle: i64, index: i64) {
    widgets::picker::set_selected(handle, index);
}

/// Get the selected item of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_get_selected(handle: i64) -> i64 {
    widgets::picker::get_selected(handle)
}

// =============================================================================
// Image
// =============================================================================

/// Create an image from a file path.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_file(path_ptr: i64) -> i64 {
    widgets::image::create_file(path_ptr as *const u8)
}

/// Create an image from a named icon/symbol.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_symbol(name_ptr: i64) -> i64 {
    widgets::image::create_symbol(name_ptr as *const u8)
}

/// #635 — Image(url, alt). Fetches the URL on a background thread via
/// WinHTTP, decodes via GDI+ GdipLoadImageFromStream from a
/// SHCreateMemStream-backed IStream, and repaints once the bytes
/// arrive (PostMessage + InvalidateRect from the worker).
#[no_mangle]
pub extern "C" fn perry_ui_image_create_url(url_ptr: i64, alt_ptr: i64) -> i64 {
    widgets::image::create_url(url_ptr as *const u8, alt_ptr as *const u8)
}

/// Set the size of an image.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_size(handle: i64, width: f64, height: f64) {
    widgets::image::set_size(handle, width, height);
}

/// Set the tint color of an image.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::image::set_tint(handle, r, g, b, a);
}

// =============================================================================
// Sheet
// =============================================================================

/// Create a sheet (modal window).
#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(width: f64, height: f64, title_val: f64) -> i64 {
    sheet::create(width, height, title_val)
}

/// Present (show) a sheet.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(sheet_handle: i64) {
    sheet::present(sheet_handle);
}

/// Dismiss (close) a sheet.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(sheet_handle: i64) {
    sheet::dismiss(sheet_handle);
}

// =============================================================================
// Toolbar
// =============================================================================

/// Create a toolbar.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    toolbar::create()
}

/// Add an item to a toolbar.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    toolbar_handle: i64,
    label_ptr: i64,
    icon_ptr: i64,
    callback: f64,
) {
    toolbar::add_item(
        toolbar_handle,
        label_ptr as *const u8,
        icon_ptr as *const u8,
        callback,
    );
}

/// Attach a toolbar to the current window.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(toolbar_handle: i64) {
    toolbar::attach(toolbar_handle);
}

// =============================================================================
// TabBar stubs (not yet implemented on Windows)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_create(_on_change: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_add_tab(_handle: i64, _label_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_set_selected(_handle: i64, _index: i64) {}

// =============================================================================
// System API
// =============================================================================

/// Open a URL in the default browser.
#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
    system::open_url(url_ptr as *const u8);
}

/// Check if dark mode is enabled.
#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    system::is_dark_mode()
}

/// Set a preference value.
#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
    system::preferences_set(key_ptr as *const u8, value);
}

/// Get a preference value.
#[no_mangle]
pub extern "C" fn perry_system_preferences_get(key_ptr: i64) -> f64 {
    system::preferences_get(key_ptr as *const u8)
}

/// Save a value to the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_save(key_ptr: i64, value_ptr: i64) {
    keychain::save(key_ptr as *const u8, value_ptr as *const u8);
}

/// Get a value from the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_get(key_ptr: i64) -> f64 {
    keychain::get(key_ptr as *const u8)
}

/// Delete a value from the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(key_ptr: i64) {
    keychain::delete(key_ptr as *const u8);
}

/// Send a desktop notification.
#[no_mangle]
pub extern "C" fn perry_system_notification_send(title_ptr: i64, body_ptr: i64) {
    system::notification_send(title_ptr as *const u8, body_ptr as *const u8);
}

/// Stub: WinRT push (PushNotificationTrigger) is a separate PR (#95
/// follow-up). Symbol exists so TS code that calls
/// `notificationRegisterRemote` links and runs without crashing.
#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(_callback: f64) {}

/// Stub: see `perry_system_notification_register_remote` above.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(_callback: f64) {}

/// Stub (#98): WNS background delivery isn't wired here yet (separate from
/// the toast pipeline). Symbol exists so cross-platform user code linking
/// against perry-ui-windows resolves cleanly. Callback is silently dropped.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(_callback: f64) {}

/// Stub: ToastNotifier.AddToSchedule wiring is a separate PR (#96 follow-up).
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _seconds: f64,
    _repeats: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_calendar(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _timestamp_ms: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_location(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _lat: f64,
    _lon: f64,
    _radius: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_system_notification_cancel(_id_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    let lang = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANGUAGE"))
        .unwrap_or_else(|_| "en".to_string());
    let code = if lang.len() >= 2 { &lang[..2] } else { "en" };
    unsafe { js_string_from_bytes(code.as_ptr(), code.len() as i64) as i64 }
}

/// Add a child widget at a specific index.
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    widgets::add_child_at(parent_handle, child_handle, index as i64);
    app::request_layout();
}

// =============================================================================
// Stubs for symbols referenced by codegen but not yet implemented on Windows
// =============================================================================

/// Set button text color.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::button::set_text_color(handle, r, g, b, a);
}

/// Set widget width (DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    let scaled = (width * app::get_dpi_scale()) as i32;
    widgets::set_fixed_width(handle, scaled);
}

/// Set widget hugging priority.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(handle: i64, priority: f64) {
    widgets::set_hugging_priority(handle, priority);
}

/// Set on-click callback (stub — not yet implemented on Windows).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_click(handle: i64, callback: f64) {
    let _ = handle;
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(
                handle: i64,
                widget_type: u8,
                callback_kind: u8,
                closure_f64: f64,
                label_ptr: *const u8,
            );
        }
        unsafe {
            perry_geisterhand_register(handle, 0, 0, callback, std::ptr::null());
        }
    }
}

/// Set widget height (fixed, DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: i64, height: f64) {
    let scaled = (height * app::get_dpi_scale()) as i32;
    widgets::set_fixed_height(handle, scaled);
}

/// Match parent height — marks the widget to stretch vertically to fill its parent.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_height(handle: i64) {
    widgets::set_match_parent_height(handle, true);
}

/// Match parent width — marks the widget to stretch horizontally to fill its parent.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_width(handle: i64) {
    widgets::set_match_parent_width(handle, true);
}

/// Set hidden state (perry_ui_widget_set_hidden — matches macOS naming convention).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
}

/// Stack: detach hidden children from layout calculation.
/// When enabled, hidden children don't occupy any space.
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_detaches_hidden(handle: i64, flag: i64) {
    widgets::set_detaches_hidden(handle, flag != 0);
}

/// Embed a native HWND into the Perry widget system.
/// Takes the HWND pointer value and returns a 1-based widget handle.
/// The widget is marked as fills_remaining so it absorbs remaining space in VStack/HStack.
#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(hwnd_ptr: i64) -> i64 {
    if hwnd_ptr == 0 {
        return 0;
    }
    #[cfg(target_os = "windows")]
    {
        let hwnd = windows::Win32::Foundation::HWND(hwnd_ptr as *mut std::ffi::c_void);
        let handle = widgets::register_widget(hwnd, widgets::WidgetKind::Canvas, 0);
        widgets::set_fills_remaining(handle, true);
        handle
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd_ptr;
        0
    }
}

/// Request location permission (stub — not available on Windows desktop).
#[no_mangle]
pub extern "C" fn perry_system_request_location(_callback: f64) {}

/// Load a plugin (stub — not yet implemented on Windows).
#[no_mangle]
pub extern "C" fn perry_plugin_load(_path_ptr: i64) -> i64 {
    0
}

/// Unload a plugin (stub — not yet implemented on Windows).
#[no_mangle]
pub extern "C" fn perry_plugin_unload(_handle: i64) {}

// NOTE: backOff, js_crypto_random_bytes_buffer, js_fetch_*, js_ws_handle_to_i64,
// and js_fetch_stream_status are provided by perry-stdlib. When linking the IDE
// (which uses both perry-stdlib and perry-ui-windows), these stubs caused
// duplicate symbol errors (LNK2005). Removed — perry-stdlib provides the real
// implementations.

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(data_ptr: i64, size: f64) -> i64 {
    widgets::qrcode::create(data_ptr as *const u8, size)
}

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(handle: i64, data_ptr: i64) {
    widgets::qrcode::set_data(handle, data_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_distribution(handle: i64, distribution: f64) {
    // Dispatch declares this as `[Widget, F64]` (matches every other platform).
    // The Windows runtime previously took `i64` — on Win64 ABI the f64 arg lands
    // in XMM1 while `i64` is read from RDX (uninitialized garbage), so the
    // distribution enum tag was random.
    widgets::set_distribution(handle, distribution as i64);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_reorder_child(_parent: i64, _child: i64, _index: i64) {}

// perry_debug_trace_init and perry_debug_trace_init_done are provided by perry_runtime

// =============================================================================
// JS interop stubs — AOT replacements for functions removed from perry-runtime
// upstream (moved to perry-jsruntime for V8 builds). These provide the correct
// AOT behavior for V8-free native builds.
// =============================================================================

/// Create a callback from a function pointer. NaN-boxes the pointer so it can
/// be stored as an f64 value and later called via js_native_call_value.
#[no_mangle]
pub extern "C" fn js_create_callback(func_ptr: i64, _closure_env: i64, _param_count: i64) -> f64 {
    perry_runtime::js_nanbox_pointer(func_ptr)
}

/// Call a JS function by module/name — no-op in AOT mode.
#[no_mangle]
pub extern "C" fn js_call_function(_module: i64, _name: i64, _args: i64, _argc: i64) -> f64 {
    f64::from_bits(perry_runtime::JSValue::undefined().bits())
}

/// Await a JS promise — in AOT mode, just pass through the value.
#[no_mangle]
pub extern "C" fn js_await_js_promise(value: f64) -> f64 {
    value
}

/// Load a JS module — no-op in AOT mode.
#[no_mangle]
pub extern "C" fn js_load_module(_path: i64) -> i64 {
    0
}

/// Construct a new instance by calling a constructor function with arguments.
#[no_mangle]
pub unsafe extern "C" fn js_new_from_handle(constructor: f64, args_ptr: i64, args_len: i64) -> f64 {
    perry_runtime::closure::js_native_call_value(
        constructor,
        args_ptr as *const f64,
        args_len as usize,
    )
}

/// Create a new instance of a class by name — no-op in pure AOT mode.
#[no_mangle]
pub extern "C" fn js_new_instance(_module: i64, _class: i64, _args: i64, _argc: i64) -> f64 {
    f64::from_bits(perry_runtime::JSValue::undefined().bits())
}

#[no_mangle]
pub extern "C" fn js_runtime_init() {}

#[no_mangle]
pub extern "C" fn js_set_property(_obj: f64, _name: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn js_get_export(_module: i64, _name: i64) -> f64 {
    f64::from_bits(perry_runtime::JSValue::undefined().bits())
}

// =============================================================================
// Additional UI stubs
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_wraps(_handle: i64, _wraps: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_get_string(handle: i64) -> i64 {
    widgets::textfield::get_string(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_submit(_handle: i64, _callback: f64) {
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
        }
        unsafe {
            perry_geisterhand_register(_handle, 1, 2, _callback, std::ptr::null());
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_next_key_view(_handle: i64, _next_handle: i64) {
    // Win32 handles tab navigation via WS_TABSTOP style (set by default)
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

// =============================================================================
// Device / screen stubs (iOS-only on macOS, stubs everywhere else)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    0.0
}

/// Layout-change callback registration — stub on every platform
/// (matches macOS shape). Real on-resize plumbing would wire WM_SIZE
/// → callback dispatch; for now apps poll dimensions via
/// `perry_get_screen_width` / `perry_get_screen_height`.
#[no_mangle]
pub extern "C" fn perry_on_layout_change(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_get_orientation() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> f64 {
    0.0
}

// Audio capture (WASAPI)
#[no_mangle]
pub extern "C" fn perry_system_audio_start() -> i64 {
    audio::start()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_stop() {
    audio::stop()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_get_level() -> f64 {
    audio::get_level()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_get_peak() -> f64 {
    audio::get_peak()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_get_waveform(count: f64) -> f64 {
    audio::get_waveform(count)
}
#[no_mangle]
pub extern "C" fn perry_system_get_device_model() -> i64 {
    audio::get_device_model()
}
/// Bug-report-flow utility: stable OS-version string. Windows stub —
/// native impl will use `GetVersionEx` / `RtlGetVersion`.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_get_os_version",
        "Windows getOSVersion (RtlGetVersion) not yet implemented",
        None,
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_audio_set_output_filename(filename_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let filename = str_from_header(filename_ptr as *const u8);
    audio::set_output_filename(filename);
}
#[no_mangle]
pub extern "C" fn perry_system_audio_start_recording() {
    audio::start_recording();
}
#[no_mangle]
pub extern "C" fn perry_system_audio_stop_recording() {
    audio::stop_recording();
}

// =============================================================================
// Splitview / VBox stubs (iOS-only layout containers)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_splitview_create() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_splitview_add_child(_handle: i64, _child: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_create(_spacing: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_add_child(_handle: i64, _child: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_finalize(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_create() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_add_child(_handle: i64, _child: i64) {}

// =============================================================================
// App icon & file open polling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(_path_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_frameless(app_handle: i64, value: f64) {
    app::app_set_frameless(app_handle, value);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_level(app_handle: i64, value_ptr: i64) {
    app::app_set_level(app_handle, value_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_transparent(app_handle: i64, value: f64) {
    app::app_set_transparent(app_handle, value);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_vibrancy(app_handle: i64, value_ptr: i64) {
    app::app_set_vibrancy(app_handle, value_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_activation_policy(app_handle: i64, value_ptr: i64) {
    app::app_set_activation_policy(app_handle, value_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    0
}

// =============================================================================
// TextArea — Win32 EDIT control with ES_MULTILINE | WS_VSCROLL
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textarea_create(on_change: f64) -> i64 {
    widgets::textarea::create(on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_set_string(handle: i64, text_ptr: i64) {
    widgets::textarea::set_string(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_get_string(handle: i64) -> i64 {
    widgets::textarea::get_string(handle)
}

// =============================================================================
// TextField focus stubs
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {}

// =============================================================================
// Stack alignment stub
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(handle: i64, alignment: f64) {
    widgets::set_alignment(handle, alignment as i64);
}

// =============================================================================
// Widget overlay & edge insets stubs
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(_parent: i64, _child: i64) {
    // For now, treat as regular add_child
    widgets::add_child(_parent, _child);
    app::request_layout();
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(
    _handle: i64,
    _x: f64,
    _y: f64,
    _w: f64,
    _h: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_edge_insets(
    handle: i64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) {
    widgets::set_insets(handle, top, left, bottom, right);
}

// =============================================================================
// LSP bridge stubs (not yet implemented on Windows)
// =============================================================================

#[no_mangle]
pub extern "C" fn hone_lsp_start(_cmd: i64, _args: i64, _cwd: i64) -> i64 {
    -1
}

#[no_mangle]
pub extern "C" fn hone_lsp_poll(_handle: i64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn hone_lsp_send(_handle: i64, _msg: i64) {}

#[no_mangle]
pub extern "C" fn hone_lsp_stop(_handle: i64) {}

// --- Camera stubs (issue #191) ---
// Real implementations live in `perry-ui-ios` and `perry-ui-android`. The
// Windows backend doesn't have a camera capture pipeline yet; these no-ops
// let user code that targets multiple platforms link cleanly.

#[no_mangle]
pub extern "C" fn perry_ui_camera_create() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_start(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_stop(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_freeze(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_unfreeze(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_sample_color(_x: f64, _y: f64) -> f64 {
    -1.0
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_set_on_tap(_handle: i64, _callback: f64) {}

// Override setjmp with a no-op stub that always returns 0.
// Perry's try/catch uses setjmp/longjmp but since we make readFileSync
// return empty string instead of throwing, longjmp is never called.
// The MSVC CRT setjmp may corrupt the stack on x64.
#[no_mangle]
pub extern "C" fn setjmp(_env: *mut i32) -> i32 {
    0
}

// --- Cross-platform toast + reactive setText stubs (Phase 2 v3.3) ---
// Full GTK4 implementation in perry-ui-gtk4. Present here so cross-platform
// code that calls showToast / setText links on Windows targets.

#[no_mangle]
pub extern "C" fn perry_ui_show_toast(_msg_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, _id_ptr: i64) -> i64 {
    perry_ui_text_create(text_ptr)
}

#[no_mangle]
pub extern "C" fn perry_ui_set_text(_id_ptr: i64, _value_ptr: i64) {}

// =============================================================================
// perry/media — streaming media playback (issue #351). AVPlayer-backed.
// See `media_playback.rs` for the implementation; everything below is a
// thin FFI thunk that the codegen-emitted `perry_media_*` declarations
// resolve to at link time.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_media_create_player(url_ptr: i64) -> i64 {
    media_playback::create_player(url_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_media_play(handle: f64) {
    media_playback::play(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_pause(handle: f64) {
    media_playback::pause(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_stop(handle: f64) {
    media_playback::stop(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_seek(handle: f64, seconds: f64) {
    media_playback::seek(handle, seconds);
}

#[no_mangle]
pub extern "C" fn perry_media_set_volume(handle: f64, volume: f64) {
    media_playback::set_volume(handle, volume);
}

#[no_mangle]
pub extern "C" fn perry_media_set_rate(handle: f64, rate: f64) {
    media_playback::set_rate(handle, rate);
}

#[no_mangle]
pub extern "C" fn perry_media_get_current_time(handle: f64) -> f64 {
    media_playback::get_current_time(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_duration(handle: f64) -> f64 {
    media_playback::get_duration(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_state(handle: f64) -> i64 {
    media_playback::get_state(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_is_playing(handle: f64) -> f64 {
    media_playback::is_playing(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_on_state_change(handle: f64, closure: f64) {
    media_playback::on_state_change(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_on_time_update(handle: f64, closure: f64) {
    media_playback::on_time_update(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_set_now_playing(
    handle: f64,
    title_ptr: i64,
    artist_ptr: i64,
    album_ptr: i64,
    artwork_ptr: i64,
) {
    media_playback::set_now_playing(
        handle,
        title_ptr as *const u8,
        artist_ptr as *const u8,
        album_ptr as *const u8,
        artwork_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_media_destroy(handle: f64) {
    media_playback::destroy(handle);
}

// =============================================================================
// Issue #553 — Windows.
//
// onScrollEnd: real impl — wired into ScrollView's existing WM_VSCROLL +
// WM_MOUSEWHEEL handlers via `check_scroll_end(handle)` after each offset
// update. Backpressure matches the macOS / iOS / GTK4 contract.
//
// BottomNavigation + ImageGallery stay stubbed on Windows: Win32 has no
// native primitives for either (no equivalent of UITabBar /
// BottomNavigationView, no UIPageViewController). A real impl would need
// either custom owner-drawn child windows (~300 lines / widget) OR a
// transition to WinUI 3 — both deferred per the existing tabbar.rs Win32
// stub convention. The symbols exist so cross-platform code links cleanly
// today and the call paths flip to real impls when WinUI lands.
//
// Pull-to-refresh on LazyVStack stays no-op: no native idiom on desktop
// Windows; explicit refresh button is the convention.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_create(on_select: f64) -> i64 {
    widgets::bottom_nav::create(on_select)
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_add_item(handle: i64, icon_ptr: i64, label_ptr: i64) {
    widgets::bottom_nav::add_item(handle, icon_ptr as *const u8, label_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_badge(handle: i64, index: i64, badge_ptr: i64) {
    widgets::bottom_nav::set_badge(handle, index, badge_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_selected(handle: i64, index: i64) {
    widgets::bottom_nav::set_selected(handle, index);
}

/// Issue #706 — Windows bottom-nav active-tab tint. State is persisted
/// on NavEntry; visual rendering waits on a future owner-drawn button
/// rewrite (Win32 standard BUTTON controls ignore WM_CTLCOLORBTN).
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::bottom_nav::set_tint_color(handle, r, g, b, a);
}

/// Issue #706 — Windows bottom-nav inactive-tabs tint.
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_unselected_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::bottom_nav::set_unselected_tint_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_refresh_control(_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_end_refreshing(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_scroll_end_callback(
    _handle: i64,
    _callback: f64,
    _threshold_items: i64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_scroll_end_callback(
    handle: i64,
    callback: f64,
    threshold_px: f64,
) {
    widgets::scrollview::set_scroll_end_callback(handle, callback, threshold_px)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_create(on_index_change: f64) -> i64 {
    widgets::image_gallery::create(on_index_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_add_image(handle: i64, url_ptr: i64, alt_ptr: i64) {
    widgets::image_gallery::add_image(handle, url_ptr as *const u8, alt_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_set_index(handle: i64, index: i64) {
    widgets::image_gallery::set_index(handle, index);
}

// --- WebView (issue #658 Phase 2) — real Win32 backend via WebView2.
//     CoreWebView2 controller hosted in a STATIC parent HWND. Async init
//     pumps the message loop synchronously so create() blocks until the
//     widget is ready. See widgets::webview for the full impl.
#[no_mangle]
pub extern "C" fn perry_ui_webview_create(
    url_ptr: i64,
    width: f64,
    height: f64,
    ephemeral: f64,
) -> i64 {
    widgets::webview::create(url_ptr as *const u8, width, height, ephemeral)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_user_agent(handle: i64, ua_ptr: i64) {
    widgets::webview::set_user_agent(handle, ua_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_allowed_domains(handle: i64, arr_handle: i64) {
    widgets::webview::set_allowed_domains(handle, arr_handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_ephemeral(handle: i64, ephemeral: i64) {
    widgets::webview::set_ephemeral(handle, ephemeral)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_should_navigate(handle: i64, closure: f64) {
    widgets::webview::set_on_should_navigate(handle, closure)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_loaded(handle: i64, closure: f64) {
    widgets::webview::set_on_loaded(handle, closure)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_error(handle: i64, closure: f64) {
    widgets::webview::set_on_error(handle, closure)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_load_url(handle: i64, url_ptr: i64) {
    widgets::webview::load_url(handle, url_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_reload(handle: i64) {
    widgets::webview::reload(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_back(handle: i64) {
    widgets::webview::go_back(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_forward(handle: i64) {
    widgets::webview::go_forward(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_can_go_back(handle: i64) -> i64 {
    widgets::webview::can_go_back(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_evaluate_js(handle: i64, js_ptr: i64, callback: f64) {
    widgets::webview::evaluate_js(handle, js_ptr as *const u8, callback)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_clear_cookies(handle: i64) {
    widgets::webview::clear_cookies(handle)
}

// AttributedText (Issue #710) — Windows RichEdit-backed.
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_create() -> i64 {
    widgets::attributed_text::create()
}
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_append(
    h: i64,
    t: i64,
    bold: i64,
    italic: i64,
    underline: i64,
    font_size: f64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::attributed_text::append(
        h,
        t as *const u8,
        bold,
        italic,
        underline,
        font_size,
        r,
        g,
        b,
        a,
    );
}
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_clear(h: i64) {
    widgets::attributed_text::clear(h);
}

// ---- In-app screen capture (issue #918) ----
/// Capture the active window as a PNG and return a base64-encoded string.
/// Returns an empty string if no window is available or capture fails.
#[no_mangle]
pub extern "C" fn perry_system_take_screenshot() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    use base64::Engine as _;
    unsafe {
        let mut len: usize = 0;
        let ptr = crate::screenshot::perry_ui_screenshot_capture(&mut len as *mut usize);
        if ptr.is_null() || len == 0 {
            return js_string_from_bytes(std::ptr::null(), 0) as i64;
        }
        let bytes = std::slice::from_raw_parts(ptr, len);
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        libc::free(ptr as *mut libc::c_void);
        js_string_from_bytes(encoded.as_ptr(), encoded.len() as i64) as i64
    }
}
