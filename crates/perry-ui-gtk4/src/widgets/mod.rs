pub mod button;
pub mod canvas;
pub mod divider;
pub mod form;
pub mod hstack;
pub mod image;
pub mod lazyvstack;
pub mod navstack;
pub mod picker;
pub mod progressview;
pub mod scrollview;
pub mod securefield;
pub mod slider;
pub mod spacer;
pub mod splitview;
pub mod text;
pub mod text_registry;
pub mod textarea;
pub mod textfield;
pub mod toast;
pub mod toggle;
pub mod vstack;
pub mod zstack;

use gtk4::prelude::*;
use gtk4::Widget;
use std::cell::RefCell;

thread_local! {
    /// Map from widget handle (1-based) to gtk4::Widget
    static WIDGETS: RefCell<Vec<Widget>> = RefCell::new(Vec::new());
}

/// Store a widget and return its handle (1-based i64).
pub fn register_widget(widget: Widget) -> i64 {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(widget);
        widgets.len() as i64
    })
}

/// Retrieve the Widget for a given handle.
pub fn get_widget(handle: i64) -> Option<Widget> {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        let idx = (handle - 1) as usize;
        widgets.get(idx).cloned()
    })
}

/// Set the hidden state of a widget.
pub fn set_hidden(handle: i64, hidden: bool) {
    if let Some(widget) = get_widget(handle) {
        widget.set_visible(!hidden);
    }
}

/// Remove all children from a container (GtkBox).
pub fn clear_children(handle: i64) {
    if let Some(parent) = get_widget(handle) {
        if let Some(container) = parent.downcast_ref::<gtk4::Box>() {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        } else if let Some(scrolled) = parent.downcast_ref::<gtk4::ScrolledWindow>() {
            scrolled.set_child(None::<&Widget>);
        } else if let Some(overlay) = parent.downcast_ref::<gtk4::Overlay>() {
            // ZStack: remove overlays
            // Note: Overlay doesn't provide iteration of overlays easily,
            // so we remove the child
            overlay.set_child(None::<&Widget>);
        } else if let Some(frame) = parent.downcast_ref::<gtk4::Frame>() {
            // Section: clear the inner box
            if let Some(inner) = frame.child() {
                if let Some(inner_box) = inner.downcast_ref::<gtk4::Box>() {
                    while let Some(child) = inner_box.first_child() {
                        inner_box.remove(&child);
                    }
                }
            }
        }
    }
}

/// Add a child widget to a parent widget at a specific index.
pub fn add_child_at(parent_handle: i64, child_handle: i64, index: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        // Unparent if already has a parent
        if child.parent().is_some() {
            child.unparent();
        }

        if let Some(container) = parent.downcast_ref::<gtk4::Box>() {
            let mut i = 0;
            let mut sibling = container.first_child();
            while i < index {
                if let Some(s) = sibling {
                    sibling = s.next_sibling();
                } else {
                    break;
                }
                i += 1;
            }
            if let Some(before) = sibling {
                child.insert_before(container, Some(&before));
            } else {
                container.append(&child);
            }
        } else {
            if let Some(container) = parent.downcast_ref::<gtk4::Box>() {
                container.append(&child);
            }
        }
    }
}

/// Add a child view to a parent view.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        // Unparent if already has a parent
        if child.parent().is_some() {
            child.unparent();
        }

        if let Some(container) = parent.downcast_ref::<gtk4::Box>() {
            // Set spacer expand direction based on parent orientation
            if child.has_css_class("perry-spacer") {
                match container.orientation() {
                    gtk4::Orientation::Horizontal => {
                        child.set_hexpand(true);
                        child.set_vexpand(false);
                    }
                    _ => {
                        child.set_vexpand(true);
                        child.set_hexpand(false);
                    }
                }
            }
            container.append(&child);
        } else if let Some(scrolled) = parent.downcast_ref::<gtk4::ScrolledWindow>() {
            scrolled.set_child(Some(&child));
        } else if let Some(overlay) = parent.downcast_ref::<gtk4::Overlay>() {
            // ZStack: first child is the main child, subsequent are overlays
            if overlay.child().is_none() {
                overlay.set_child(Some(&child));
            } else {
                overlay.add_overlay(&child);
            }
        } else if let Some(frame) = parent.downcast_ref::<gtk4::Frame>() {
            // Section: add to the inner box
            if let Some(inner) = frame.child() {
                if let Some(inner_box) = inner.downcast_ref::<gtk4::Box>() {
                    inner_box.append(&child);
                }
            }
        } else {
            eprintln!("perry-ui-gtk4: add_child called on unsupported parent type");
        }
    }
}

/// Apply inline CSS to a single widget via a per-widget CssProvider.
pub fn apply_css(widget: &Widget, css: &str) {
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(css);
    widget
        .style_context()
        .add_provider(&provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
}

/// Set enabled/disabled state of a widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    if let Some(widget) = get_widget(handle) {
        widget.set_sensitive(enabled);
    }
}

/// Set opacity (issue #185 Phase B). GTK4 has a built-in
/// `Widget::set_opacity(0.0..=1.0)` that handles compositing on the
/// platform's behalf; no CSS provider needed.
pub fn set_opacity(handle: i64, opacity: f64) {
    if let Some(widget) = get_widget(handle) {
        widget.set_opacity(opacity);
    }
}

thread_local! {
    /// Joint border state per widget handle: `(color, width)`. Both
    /// setters update this and re-emit the CSS rule together, because
    /// CSS requires `border-style: solid` + a non-zero width + a color
    /// all in the same provider for a border to actually render.
    /// Setting only one would otherwise be silently ignored.
    static BORDER_STATE: RefCell<std::collections::HashMap<i64, (Option<(f64, f64, f64, f64)>, Option<f64>)>>
        = RefCell::new(std::collections::HashMap::new());
}

/// Helper: regenerate the per-handle border CSS provider from current
/// state. Called from both `set_border_color` and `set_border_width`.
fn apply_border_css(handle: i64) {
    let Some(widget) = get_widget(handle) else {
        return;
    };
    let (color, width) =
        BORDER_STATE.with(|s| s.borrow().get(&handle).copied().unwrap_or((None, None)));
    // Defaults match CALayer-ish behavior: width 1.0 if unset,
    // color black if unset. The cross-platform shape lets users
    // call either setter alone and still get a visible border.
    let (r, g, b, a) = color.unwrap_or((0.0, 0.0, 0.0, 1.0));
    let w = width.unwrap_or(1.0);
    let class_name = format!("perry-bd-{}", handle);
    widget.remove_css_class(&class_name);
    let rgba = format!(
        "rgba({},{},{},{})",
        (r * 255.0) as i32,
        (g * 255.0) as i32,
        (b * 255.0) as i32,
        a
    );
    let decl = format!("border: {}px solid {};", w as i32, rgba);
    let is_button = widget.downcast_ref::<gtk4::Button>().is_some();
    if is_button {
        widget.add_css_class(&class_name);
        let css = format!(
            "button.flat.{} {{ {} }}\nbutton.{} {{ {} }}",
            class_name, decl, class_name, decl
        );
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&css);
        gtk4::style_context_add_provider_for_display(
            &widget.display(),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    } else {
        let css = format!("* {{ {} }}", decl);
        apply_css(&widget, &css);
    }
}

/// Set border color (issue #185 Phase B). Joint state with `set_border_width`.
pub fn set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    BORDER_STATE.with(|s| {
        let mut state = s.borrow_mut();
        let entry = state.entry(handle).or_insert((None, None));
        entry.0 = Some((r, g, b, a));
    });
    apply_border_css(handle);
}

/// Set border width (issue #185 Phase B). Joint state with `set_border_color`.
pub fn set_border_width(handle: i64, width: f64) {
    BORDER_STATE.with(|s| {
        let mut state = s.borrow_mut();
        let entry = state.entry(handle).or_insert((None, None));
        entry.1 = Some(width);
    });
    apply_border_css(handle);
}

/// Set tooltip text on a widget.
pub fn set_tooltip(handle: i64, text_ptr: *const u8) {
    let text = crate::app::str_from_header(text_ptr);
    if let Some(widget) = get_widget(handle) {
        widget.set_tooltip_text(Some(text));
    }
}

/// Set control size via CSS classes (perry-small, perry-mini, perry-large).
pub fn set_control_size(handle: i64, size: i64) {
    if let Some(widget) = get_widget(handle) {
        widget.remove_css_class("perry-small");
        widget.remove_css_class("perry-mini");
        widget.remove_css_class("perry-large");
        widget.remove_css_class("perry-regular");
        match size {
            0 => widget.add_css_class("perry-mini"),
            1 => widget.add_css_class("perry-small"),
            2 => widget.add_css_class("perry-regular"),
            3 => widget.add_css_class("perry-large"),
            _ => {}
        }
    }
}

/// Set corner radius on a widget via CSS.
pub fn set_corner_radius(handle: i64, radius: f64) {
    if let Some(widget) = get_widget(handle) {
        let is_button = widget.downcast_ref::<gtk4::Button>().is_some();
        if is_button {
            let class_name = format!("perry-cr-{}", handle);
            widget.add_css_class(&class_name);
            let css = format!(
                "button.flat.{} {{ border-radius: {}px; }}\n\
                 button.{} {{ border-radius: {}px; }}",
                class_name, radius as i32, class_name, radius as i32
            );
            let provider = gtk4::CssProvider::new();
            provider.load_from_data(&css);
            gtk4::style_context_add_provider_for_display(
                &widget.display(),
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_USER,
            );
        } else {
            let css = format!("* {{ border-radius: {}px; }}", radius as i32);
            apply_css(&widget, &css);
        }
    }
}

/// Set drop shadow on a widget via CSS `box-shadow` (issue #185 Phase B).
/// `(r, g, b, a)` is shadow color in 0–1 (alpha rides on the rgba() in CSS,
/// so a non-1 alpha produces the soft tint just like the Apple twin's
/// shadowOpacity). `blur` is the blur radius (px). `(offset_x, offset_y)`
/// is the offset (positive y = downward, matching CSS `box-shadow` and
/// the Apple CALayer twin).
///
/// Pattern mirrors `set_corner_radius`: a per-handle CSS class like
/// `perry-sh-{handle}` is added to the widget, and a fresh
/// `CssProvider` emits the `box-shadow` rule scoped to that class.
/// Display-level `STYLE_PROVIDER_PRIORITY_USER` for buttons (matches
/// the corner-radius button special-case), widget-level for other
/// widgets via the shared `apply_css` helper. The class is removed
/// before re-adding so repeat calls don't pile up stale providers.
pub fn set_shadow(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
) {
    if let Some(widget) = get_widget(handle) {
        let class_name = format!("perry-sh-{}", handle);
        widget.remove_css_class(&class_name);
        let r255 = (r * 255.0) as i32;
        let g255 = (g * 255.0) as i32;
        let b255 = (b * 255.0) as i32;
        let rgba = format!("rgba({},{},{},{})", r255, g255, b255, a);
        let shadow_decl = format!(
            "box-shadow: {}px {}px {}px {};",
            offset_x as i32, offset_y as i32, blur as i32, rgba
        );
        let is_button = widget.downcast_ref::<gtk4::Button>().is_some();
        if is_button {
            widget.add_css_class(&class_name);
            let css = format!(
                "button.flat.{} {{ {} }}\n\
                 button.{} {{ {} }}",
                class_name, shadow_decl, class_name, shadow_decl
            );
            let provider = gtk4::CssProvider::new();
            provider.load_from_data(&css);
            gtk4::style_context_add_provider_for_display(
                &widget.display(),
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_USER,
            );
        } else {
            let css = format!("* {{ {} }}", shadow_decl);
            apply_css(&widget, &css);
        }
    }
}

/// Set background color on a widget via CSS.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(widget) = get_widget(handle) {
        let rgba = format!(
            "rgba({},{},{},{})",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            a
        );
        // For buttons, make flat first (strips Adwaita chrome), then set background
        // via a unique CSS class on the display provider at USER priority.
        let is_button = widget.downcast_ref::<gtk4::Button>().is_some();
        if is_button {
            // Make flat to strip Adwaita chrome, then apply background via
            // display-level CSS at USER priority to override the theme.
            widget.add_css_class("flat");
            let class_name = format!("perry-bg-{}", handle);
            widget.add_css_class(&class_name);
            let css = format!(
                "button.flat.{} {{ background-color: {}; background-image: none; }}\n\
                 button.flat.{}:hover {{ background-color: {}; background-image: none; }}",
                class_name, rgba, class_name, rgba
            );
            let provider = gtk4::CssProvider::new();
            provider.load_from_data(&css);
            gtk4::style_context_add_provider_for_display(
                &widget.display(),
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_USER,
            );
        } else {
            let css = format!("* {{ background: {}; background-color: {}; }}", rgba, rgba);
            apply_css(&widget, &css);
        }
    }
}

/// Set a linear gradient background on a widget via CSS.
pub fn set_background_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    _a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    _a2: f64,
    direction: f64,
) {
    if let Some(widget) = get_widget(handle) {
        let angle = if direction < 0.5 {
            "to bottom"
        } else {
            "to right"
        };
        let css = format!(
            "* {{ background: linear-gradient({}, rgb({},{},{}), rgb({},{},{})); }}",
            angle,
            (r1 * 255.0) as u8,
            (g1 * 255.0) as u8,
            (b1 * 255.0) as u8,
            (r2 * 255.0) as u8,
            (g2 * 255.0) as u8,
            (b2 * 255.0) as u8,
        );
        apply_css(&widget, &css);
    }
}

/// Set an on-hover callback on a widget.
pub fn set_on_hover(handle: i64, callback: f64) {
    extern "C" {
        fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    if let Some(widget) = get_widget(handle) {
        let motion = gtk4::EventControllerMotion::new();
        let cb = callback;
        motion.connect_enter(move |_ctrl, _x, _y| {
            let ptr = unsafe { js_nanbox_get_pointer(cb) } as *const u8;
            unsafe {
                js_closure_call1(ptr, 1.0);
            }
        });
        let cb2 = callback;
        motion.connect_leave(move |_ctrl| {
            let ptr = unsafe { js_nanbox_get_pointer(cb2) } as *const u8;
            unsafe {
                js_closure_call1(ptr, 0.0);
            }
        });
        widget.add_controller(motion);
    }
}

/// Set a double-click callback on a widget.
pub fn set_on_double_click(handle: i64, callback: f64) {
    extern "C" {
        fn js_closure_call0(closure: *const u8) -> f64;
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    if let Some(widget) = get_widget(handle) {
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(1);
        let cb = callback;
        gesture.connect_pressed(move |_gesture, n_press, _x, _y| {
            if n_press == 2 {
                let ptr = unsafe { js_nanbox_get_pointer(cb) } as *const u8;
                unsafe {
                    js_closure_call0(ptr);
                }
            }
        });
        widget.add_controller(gesture);
    }
}

/// Set a single-click callback on a widget.
pub fn set_on_click(handle: i64, callback: f64) {
    extern "C" {
        fn js_closure_call0(closure: *const u8) -> f64;
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    if let Some(widget) = get_widget(handle) {
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(1);
        let cb = callback;
        gesture.connect_pressed(move |_gesture, n_press, _x, _y| {
            if n_press == 1 {
                let ptr = unsafe { js_nanbox_get_pointer(cb) } as *const u8;
                unsafe {
                    js_closure_call0(ptr);
                }
            }
        });
        widget.add_controller(gesture);
    }
}

/// GTK4 already excludes non-visible children from layout — this is a no-op stub.
pub fn set_detaches_hidden(_handle: i64, _detaches: bool) {}

/// Animate the opacity of a widget. `duration_secs` is in seconds.
pub fn animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    use gtk4::glib;
    if let Some(widget) = get_widget(handle) {
        let start = widget.opacity();
        let steps = ((duration_secs * 1000.0) / 16.0).max(1.0) as i32;
        let delta = (target - start) / steps as f64;
        let step_count = std::cell::Cell::new(0);
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let i = step_count.get() + 1;
            step_count.set(i);
            if i >= steps {
                widget.set_opacity(target);
                glib::ControlFlow::Break
            } else {
                widget.set_opacity(start + delta * i as f64);
                glib::ControlFlow::Continue
            }
        });
    }
}

/// Set a fixed width on a widget using set_size_request.
pub fn set_width(handle: i64, width: f64) {
    if let Some(widget) = get_widget(handle) {
        widget.set_size_request(width as i32, -1);
    }
}

/// Set content hugging priority: high priority (≥249) → don't hexpand; low → do hexpand.
pub fn set_hugging_priority(handle: i64, priority: f64) {
    if let Some(widget) = get_widget(handle) {
        widget.set_hexpand(priority < 249.0);
    }
}

/// Set edge insets (padding) on a widget via CSS padding.
/// This matches macOS behavior where edgeInsets is INTERNAL padding,
/// unlike GTK4 margins which are external.
pub fn set_edge_insets(handle: i64, top: f64, left: f64, bottom: f64, right: f64) {
    if let Some(widget) = get_widget(handle) {
        let css = format!(
            "* {{ padding: {}px {}px {}px {}px; }}",
            top as i32, right as i32, bottom as i32, left as i32
        );
        apply_css(&widget, &css);
    }
}

/// Make a widget expand to fill its parent's width.
pub fn match_parent_width(handle: i64) {
    if let Some(widget) = get_widget(handle) {
        widget.set_hexpand(true);
        widget.set_halign(gtk4::Align::Fill);
    }
}

/// Make a widget expand to fill its parent's height.
pub fn match_parent_height(handle: i64) {
    if let Some(widget) = get_widget(handle) {
        widget.set_vexpand(true);
        widget.set_valign(gtk4::Align::Fill);
    }
}

/// Set a fixed height on a widget via CSS min/max-height + valign constraint.
pub fn set_height(handle: i64, height: f64) {
    if let Some(widget) = get_widget(handle) {
        let h = height as i32;
        // Use CSS to enforce both min and max height
        let css = format!("* {{ min-height: {}px; max-height: {}px; }}", h, h);
        apply_css(&widget, &css);
        // Prevent vertical expansion beyond the requested height
        widget.set_vexpand(false);
        widget.set_valign(gtk4::Align::Center);
    }
}

/// Set distribution on a GtkBox (stack).
/// 0 = Fill (default), 1 = FillEqually (homogeneous).
pub fn set_distribution(handle: i64, distribution: i64) {
    if let Some(widget) = get_widget(handle) {
        if let Some(container) = widget.downcast_ref::<gtk4::Box>() {
            container.set_homogeneous(distribution == 1);
        }
    }
}

/// Set alignment on a GtkBox (stack).
/// Maps macOS NSLayoutAttribute values to GTK4 Align:
/// 5 (Leading) → Start, 9 (CenterX) → Center, 7 (Width/Fill) → Fill
/// 3 (Top) → Start, 12 (CenterY) → Center, 4 (Bottom) → End
pub fn set_alignment(handle: i64, alignment: i64) {
    if let Some(widget) = get_widget(handle) {
        if let Some(container) = widget.downcast_ref::<gtk4::Box>() {
            let is_vertical = container.orientation() == gtk4::Orientation::Vertical;
            if is_vertical {
                // Vertical stack: alignment controls children's horizontal alignment
                let align = match alignment {
                    5 => gtk4::Align::Start,  // Leading
                    9 => gtk4::Align::Center, // CenterX
                    7 => gtk4::Align::Fill,   // Width
                    _ => gtk4::Align::Fill,
                };
                // Set halign on all existing children
                let mut child = container.first_child();
                while let Some(c) = child {
                    c.set_halign(align);
                    child = c.next_sibling();
                }
            } else {
                // Horizontal stack: alignment controls children's vertical alignment
                let align = match alignment {
                    3 => gtk4::Align::Start,   // Top
                    12 => gtk4::Align::Center, // CenterY
                    4 => gtk4::Align::End,     // Bottom
                    _ => gtk4::Align::Fill,
                };
                let mut child = container.first_child();
                while let Some(c) = child {
                    c.set_valign(align);
                    child = c.next_sibling();
                }
            }
        }
    }
}

/// Remove a single child widget from its parent. Mirrors macOS
/// `perry_ui_widget_remove_child`. Dispatches by parent container kind:
/// Box uses `remove(&child)`; ScrolledWindow / Frame inner-box / Overlay
/// each clear by their own API. The handle stays registered (we don't
/// shrink the WIDGETS vec, since handles are positional indices) — only
/// the GTK4 parent link is severed, mirroring NSView's
/// `removeFromSuperview`.
pub fn remove_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        if let Some(container) = parent.downcast_ref::<gtk4::Box>() {
            if child.parent().as_ref() == Some(container.upcast_ref::<Widget>()) {
                container.remove(&child);
            }
        } else if let Some(scrolled) = parent.downcast_ref::<gtk4::ScrolledWindow>() {
            if scrolled.child().as_ref() == Some(&child) {
                scrolled.set_child(None::<&Widget>);
            }
        } else if let Some(overlay) = parent.downcast_ref::<gtk4::Overlay>() {
            if overlay.child().as_ref() == Some(&child) {
                overlay.set_child(None::<&Widget>);
            } else {
                overlay.remove_overlay(&child);
            }
        } else if let Some(frame) = parent.downcast_ref::<gtk4::Frame>() {
            if let Some(inner) = frame.child() {
                if let Some(inner_box) = inner.downcast_ref::<gtk4::Box>() {
                    if child.parent().as_ref() == Some(inner_box.upcast_ref::<Widget>()) {
                        inner_box.remove(&child);
                    }
                }
            }
        } else if child.parent().is_some() {
            child.unparent();
        }
    }
}

/// Reorder a child within its parent container by positional index.
/// Mirrors macOS `perry_ui_widget_reorder_child(parent, from, to)` —
/// the macOS impl walks `arrangedSubviews` and uses `insertArrangedSubview:atIndex:`.
/// On GTK4 we walk `parent.first_child()` siblings to locate the child at
/// `from_index`, then walk again to find the anchor sibling at `to_index`,
/// and call `Box::reorder_child_after(&child, anchor)`. Out-of-range
/// indices are clamped: from > N-1 → no-op; to >= N → moves to the end.
pub fn reorder_child(parent_handle: i64, from_index: i64, to_index: i64) {
    let Some(parent) = get_widget(parent_handle) else {
        return;
    };
    let Some(container) = parent.downcast_ref::<gtk4::Box>() else {
        return;
    };

    // Snapshot the sibling list (positional, before mutation).
    let mut siblings: Vec<Widget> = Vec::new();
    let mut cur = container.first_child();
    while let Some(c) = cur {
        siblings.push(c.clone());
        cur = c.next_sibling();
    }
    let n = siblings.len() as i64;
    if from_index < 0 || from_index >= n {
        return;
    }
    let child = siblings[from_index as usize].clone();
    let to = to_index.clamp(0, n - 1);
    if to == from_index {
        return;
    }
    // `reorder_child_after(child, sibling)` — sibling=None places child first.
    if to == 0 {
        container.reorder_child_after(&child, None::<&Widget>);
    } else {
        // The anchor is the sibling that should end up immediately *before* child.
        // After removal of child from position `from`, the sibling currently at
        // `to` (when moving forward) or `to-1` (when moving back) is the right anchor.
        let anchor_idx = if to > from_index {
            to as usize
        } else {
            (to - 1).max(0) as usize
        };
        let anchor = siblings[anchor_idx].clone();
        container.reorder_child_after(&child, Some(&anchor));
    }
}

/// Add an overlay child on top of a parent. Mirrors macOS
/// `perry_ui_widget_add_overlay` (which uses plain `addSubview`, so the
/// child floats above arranged subviews). On GTK4 the natural primitive
/// is `gtk4::Overlay::add_overlay`. If the parent isn't already an
/// `Overlay`, we cannot retroactively wrap it in one (GTK4 widgets have a
/// single immutable parent slot), so we log a warning and fall through to
/// `add_child` — the user will still see their widget, just not floating
/// above siblings. Use a `ZStack` (which is backed by `gtk4::Overlay`) as
/// the parent for true overlay semantics.
pub fn add_overlay(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        if child.parent().is_some() {
            child.unparent();
        }
        if let Some(overlay) = parent.downcast_ref::<gtk4::Overlay>() {
            overlay.add_overlay(&child);
        } else {
            eprintln!(
                "perry-ui-gtk4: widget_add_overlay on non-Overlay parent — \
                 falling back to add_child. Wrap the parent in a ZStack for true overlay."
            );
            add_child(parent_handle, child_handle);
        }
    }
}

/// Position + size an overlay child. Mirrors macOS
/// `perry_ui_widget_set_overlay_frame` (CGRect on a subview). GTK4's
/// layout model is constraint-based, not absolute-frame, so we approximate
/// using `halign/valign = Start` + start/top margins for the (x, y) offset
/// and `set_size_request(w, h)` for the size. This works correctly when
/// the parent is a `gtk4::Overlay` (the common case for floating widgets)
/// because Overlay honors child halign/valign. For other parent types the
/// approximation may not produce pixel-perfect positioning — true
/// absolute-frame semantics on a non-Overlay parent need a `gtk4::Fixed`
/// wrapper, deferred until a use case surfaces.
pub fn set_overlay_frame(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    if let Some(widget) = get_widget(handle) {
        widget.set_halign(gtk4::Align::Start);
        widget.set_valign(gtk4::Align::Start);
        widget.set_margin_start(x as i32);
        widget.set_margin_top(y as i32);
        widget.set_size_request(w as i32, h as i32);
    }
}

/// Animate the position of a widget (via margin offset). `duration_secs` is in seconds.
pub fn animate_position(handle: i64, dx: f64, dy: f64, duration_secs: f64) {
    use gtk4::glib;
    if let Some(widget) = get_widget(handle) {
        let start_x = widget.margin_start() as f64;
        let start_y = widget.margin_top() as f64;
        let steps = ((duration_secs * 1000.0) / 16.0).max(1.0) as i32;
        let step_count = std::cell::Cell::new(0);
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let i = step_count.get() + 1;
            step_count.set(i);
            let t = i as f64 / steps as f64;
            if i >= steps {
                widget.set_margin_start((start_x + dx) as i32);
                widget.set_margin_top((start_y + dy) as i32);
                glib::ControlFlow::Break
            } else {
                widget.set_margin_start((start_x + dx * t) as i32);
                widget.set_margin_top((start_y + dy * t) as i32);
                glib::ControlFlow::Continue
            }
        });
    }
}
