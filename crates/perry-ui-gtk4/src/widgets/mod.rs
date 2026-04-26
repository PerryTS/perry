pub mod text;
pub mod button;
pub mod vstack;
pub mod hstack;
pub mod spacer;
pub mod divider;
pub mod textarea;
pub mod textfield;
pub mod toggle;
pub mod slider;
pub mod scrollview;
pub mod securefield;
pub mod progressview;
pub mod form;
pub mod zstack;
pub mod picker;
pub mod canvas;
pub mod navstack;
pub mod lazyvstack;
pub mod image;

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
    widget.style_context().add_provider(&provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
}

/// Set enabled/disabled state of a widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    if let Some(widget) = get_widget(handle) {
        widget.set_sensitive(enabled);
    }
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
                class_name, radius as i32,
                class_name, radius as i32
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
            let css = format!(
                "* {{ background: {}; background-color: {}; }}",
                rgba, rgba
            );
            apply_css(&widget, &css);
        }
    }
}

/// Set a linear gradient background on a widget via CSS.
pub fn set_background_gradient(handle: i64, r1: f64, g1: f64, b1: f64, _a1: f64, r2: f64, g2: f64, b2: f64, _a2: f64, direction: f64) {
    if let Some(widget) = get_widget(handle) {
        let angle = if direction < 0.5 { "to bottom" } else { "to right" };
        let css = format!(
            "* {{ background: linear-gradient({}, rgb({},{},{}), rgb({},{},{})); }}",
            angle,
            (r1 * 255.0) as u8, (g1 * 255.0) as u8, (b1 * 255.0) as u8,
            (r2 * 255.0) as u8, (g2 * 255.0) as u8, (b2 * 255.0) as u8,
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
            unsafe { js_closure_call1(ptr, 1.0); }
        });
        let cb2 = callback;
        motion.connect_leave(move |_ctrl| {
            let ptr = unsafe { js_nanbox_get_pointer(cb2) } as *const u8;
            unsafe { js_closure_call1(ptr, 0.0); }
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
                unsafe { js_closure_call0(ptr); }
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
                unsafe { js_closure_call0(ptr); }
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
                    5 => gtk4::Align::Start,   // Leading
                    9 => gtk4::Align::Center,  // CenterX
                    7 => gtk4::Align::Fill,    // Width
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
