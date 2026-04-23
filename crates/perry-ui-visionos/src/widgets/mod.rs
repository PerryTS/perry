pub mod text;
pub mod button;
pub mod vstack;
pub mod hstack;
pub mod spacer;
pub mod divider;
pub mod textarea;
pub mod textfield;
pub mod securefield;
pub mod toggle;
pub mod slider;
pub mod scrollview;
pub mod canvas;
pub mod progressview;
pub mod image;
pub mod picker;
pub mod form;
pub mod navstack;
pub mod zstack;
pub mod tabbar;
pub mod qrcode;
pub mod splitview;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSObjectProtocol;
use objc2_ui_kit::{UIView, UIStackView};
use std::cell::RefCell;
use std::ffi::c_void;

thread_local! {
    /// Map from widget handle (1-based) to UIView
    static WIDGETS: RefCell<Vec<Retained<UIView>>> = RefCell::new(Vec::new());
    /// Stored height constraints per widget handle, so set_height can update instead of duplicate.
    static HEIGHT_CONSTRAINTS: RefCell<std::collections::HashMap<i64, Retained<AnyObject>>> = RefCell::new(std::collections::HashMap::new());
}

/// Store a UIView and return its handle (1-based i64).
pub fn register_widget(view: Retained<UIView>) -> i64 {
    let handle = WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(view.clone());
        widgets.len() as i64
    });
    #[cfg(feature = "geisterhand")]
    {
        use objc2_foundation::NSString;
        let id_str = format!("gh-{}", handle);
        let ns_id = NSString::from_str(&id_str);
        unsafe {
            let _: () = objc2::msg_send![&*view, setAccessibilityIdentifier: &*ns_id];
        }
    }
    handle
}

#[cfg(feature = "geisterhand")]
fn alloc_string_result(s: &str, out_len: *mut usize) -> *mut u8 {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let buf = unsafe { libc::malloc(len) as *mut u8 };
    if buf.is_null() { return std::ptr::null_mut(); }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
        *out_len = len;
    }
    buf
}

/// Read the current value of a widget by handle (UITextField → text, UISlider → value, UISwitch → isOn).
#[cfg(feature = "geisterhand")]
#[no_mangle]
pub extern "C" fn perry_ui_read_widget_value(handle: i64, out_len: *mut usize) -> *mut u8 {
    unsafe { *out_len = 0; }
    if let Some(view) = get_widget(handle) {
        unsafe {
            if let Some(tf_cls) = objc2::runtime::AnyClass::get(c"UITextField") {
                let is_tf: bool = objc2::msg_send![&*view, isKindOfClass: tf_cls];
                if is_tf {
                    let val: *const objc2::runtime::AnyObject = objc2::msg_send![&*view, text];
                    if !val.is_null() {
                        let ns: &objc2_foundation::NSString = &*(val as *const objc2_foundation::NSString);
                        return alloc_string_result(&ns.to_string(), out_len);
                    }
                }
            }
            if let Some(slider_cls) = objc2::runtime::AnyClass::get(c"UISlider") {
                let is_slider: bool = objc2::msg_send![&*view, isKindOfClass: slider_cls];
                if is_slider {
                    let val: f32 = objc2::msg_send![&*view, value];
                    return alloc_string_result(&format!("{}", val), out_len);
                }
            }
            if let Some(switch_cls) = objc2::runtime::AnyClass::get(c"UISwitch") {
                let is_switch: bool = objc2::msg_send![&*view, isKindOfClass: switch_cls];
                if is_switch {
                    let on: bool = objc2::msg_send![&*view, isOn];
                    return alloc_string_result(if on { "true" } else { "false" }, out_len);
                }
            }
        }
    }
    std::ptr::null_mut()
}

/// Query all widgets and return JSON with handle, visible, and frame data.
#[cfg(feature = "geisterhand")]
#[no_mangle]
pub extern "C" fn perry_ui_query_widget_tree(out_len: *mut usize) -> *mut u8 {
    use objc2_core_foundation::CGRect;
    let json = WIDGETS.with(|w| {
        let widgets = w.borrow();
        let mut s = String::from("[");
        for (i, view) in widgets.iter().enumerate() {
            let handle = (i + 1) as i64;
            if i > 0 { s.push(','); }
            unsafe {
                let hidden: bool = objc2::msg_send![&**view, isHidden];
                let visible = !hidden;
                let frame: CGRect = objc2::msg_send![&**view, frame];
                s.push_str(&format!(
                    r#"{{"handle":{},"visible":{},"frame":{{"x":{:.0},"y":{:.0},"width":{:.0},"height":{:.0}}}}}"#,
                    handle, visible, frame.origin.x, frame.origin.y, frame.size.width, frame.size.height
                ));
            }
        }
        s.push(']');
        s
    });
    let bytes = json.into_bytes();
    let len = bytes.len();
    let buf = unsafe { libc::malloc(len) as *mut u8 };
    if buf.is_null() {
        unsafe { *out_len = 0; }
        return std::ptr::null_mut();
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
        *out_len = len;
    }
    buf
}

/// Retrieve the UIView for a given handle.
pub fn get_widget(handle: i64) -> Option<Retained<UIView>> {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        let idx = (handle - 1) as usize;
        widgets.get(idx).cloned()
    })
}

/// Set the hidden state of a widget.
pub fn set_hidden(handle: i64, hidden: bool) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setHidden: hidden];
        }
    }
}

/// Remove all arranged subviews from a container (UIStackView).
/// Safe implementation: snapshots subviews before mutation, removes in reverse
/// order, and cleans up HEIGHT_CONSTRAINTS for removed widgets.
pub fn clear_children(handle: i64) {
    if let Some(parent) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"UIStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let stack: &UIStackView = unsafe { &*(Retained::as_ptr(&parent) as *const UIStackView) };

            // Phase 1: Snapshot subviews into a Vec (avoid iterator invalidation)
            let subviews = stack.arrangedSubviews();
            let count = subviews.len();
            let mut views: Vec<Retained<UIView>> = Vec::with_capacity(count);
            for i in 0..count {
                let sv: *const UIView = unsafe { objc2::msg_send![&subviews, objectAtIndex: i] };
                if !sv.is_null() {
                    if let Some(retained) = unsafe { Retained::retain(sv as *mut UIView) } {
                        views.push(retained);
                    }
                }
            }

            // Phase 2: Collect handles for cleanup
            let handles: Vec<i64> = views.iter().filter_map(|sv| {
                WIDGETS.with(|w| {
                    let widgets = w.borrow();
                    for (idx, widget) in widgets.iter().enumerate() {
                        if std::ptr::eq(Retained::as_ptr(widget), &**sv as *const UIView) {
                            return Some((idx + 1) as i64);
                        }
                    }
                    None
                })
            }).collect();

            // Phase 3: Remove views in reverse order
            for sv in views.iter().rev() {
                unsafe {
                    let _: () = objc2::msg_send![stack, removeArrangedSubview: &**sv];
                    sv.removeFromSuperview();
                }
            }

            // Phase 4: Clean up HEIGHT_CONSTRAINTS for removed widgets
            for h in &handles {
                HEIGHT_CONSTRAINTS.with(|hc| {
                    if let Some(old) = hc.borrow_mut().remove(h) {
                        unsafe { let _: () = objc2::msg_send![&*old, setActive: false]; }
                    }
                });
            }
        }
    }
}

/// Add a child view to a parent view at a specific index.
pub fn add_child_at(parent_handle: i64, child_handle: i64, index: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let is_stack = if let Some(cls) = AnyClass::get(c"UIStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &UIStackView = unsafe { &*(Retained::as_ptr(&parent) as *const UIStackView) };
            unsafe {
                let _: () = objc2::msg_send![stack, insertArrangedSubview: &*child, atIndex: index as usize];
            }
        } else {
            parent.addSubview(&child);
        }
    }
}

/// Add a child view to a parent view.
/// If the parent is a UIStackView, uses addArrangedSubview for proper layout.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let is_stack = if let Some(cls) = AnyClass::get(c"UIStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &UIStackView = unsafe { &*(Retained::as_ptr(&parent) as *const UIStackView) };
            stack.addArrangedSubview(&child);
        } else {
            // Non-stack parent (e.g. ZStack/plain UIView): pin child to fill parent
            zstack::add_child(parent_handle, child_handle);
        }
    }
}

// =============================================================================
// Widget Styling (Background, Gradient, Corner Radius)
// =============================================================================

type CGFloat = f64;

extern "C" {
    fn CGColorRelease(color: *mut c_void);
    fn CGColorCreateSRGB(red: CGFloat, green: CGFloat, blue: CGFloat, alpha: CGFloat) -> *mut c_void;
}

/// Create a retained CGColor from RGBA using CGColorCreateSRGB (iOS 14+).
/// Returns a +1 retained CGColorRef that must be released with CGColorRelease.
pub unsafe fn create_cg_color(r: f64, g: f64, b: f64, a: f64) -> *mut c_void {
    CGColorCreateSRGB(r, g, b, a)
}

/// Set a solid background color on any widget.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let ui_color: Retained<AnyObject> = objc2::msg_send![
                AnyClass::get(c"UIColor").unwrap(),
                colorWithRed: r,
                green: g,
                blue: b,
                alpha: a
            ];
            let _: () = objc2::msg_send![&*view, setBackgroundColor: &*ui_color];
        }
    }
}

/// Register a dynamic UIView subclass whose `layoutSubviews` keeps all sublayers
/// sized to the view's layer bounds. Created once via ObjC runtime.
fn gradient_bg_class() -> &'static AnyClass {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        unsafe {
            extern "C" {
                fn objc_allocateClassPair(
                    superclass: *const AnyClass, name: *const std::ffi::c_char, extra: usize
                ) -> *mut AnyClass;
                fn objc_registerClassPair(cls: *mut AnyClass);
                fn class_addMethod(
                    cls: *mut AnyClass, sel: objc2::runtime::Sel,
                    imp: *const std::ffi::c_void, types: *const std::ffi::c_char,
                ) -> bool;
            }
            let superclass = AnyClass::get(c"UIView").unwrap();
            let cls = objc_allocateClassPair(superclass, c"PerryGradientBGView".as_ptr(), 0);
            assert!(!cls.is_null(), "Failed to allocate PerryGradientBGView class");

            // Override layoutSubviews to resize all sublayers to layer.bounds
            extern "C" fn layout_subviews(this: &AnyObject, _cmd: objc2::runtime::Sel) {
                unsafe {
                    // Call [super layoutSubviews]
                    let sup = AnyClass::get(c"UIView").unwrap();
                    let _: () = objc2::msg_send![super(this, sup), layoutSubviews];
                    // Resize all sublayers to match layer bounds
                    let layer: *mut AnyObject = objc2::msg_send![this, layer];
                    if layer.is_null() { return; }
                    let bounds: objc2_core_foundation::CGRect = objc2::msg_send![layer, bounds];
                    let sublayers: *mut AnyObject = objc2::msg_send![layer, sublayers];
                    if !sublayers.is_null() {
                        let count: usize = objc2::msg_send![sublayers, count];
                        for i in 0..count {
                            let sub: *mut AnyObject = objc2::msg_send![sublayers, objectAtIndex: i];
                            let _: () = objc2::msg_send![sub, setFrame: bounds];
                        }
                    }
                }
            }
            class_addMethod(
                cls,
                objc2::sel!(layoutSubviews),
                layout_subviews as *const std::ffi::c_void,
                c"v@:".as_ptr(),
            );
            objc_registerClassPair(cls);
        }
    });
    AnyClass::get(c"PerryGradientBGView").unwrap()
}

/// Set a linear gradient background on any widget.
/// Uses a background UIView subclass (with layoutSubviews override) pinned via Auto Layout.
/// This ensures the CAGradientLayer always matches the view bounds, even when the
/// parent starts at zero size (common during init before Auto Layout resolves).
pub fn set_background_gradient(
    handle: i64, r1: f64, g1: f64, b1: f64, a1: f64,
    r2: f64, g2: f64, b2: f64, a2: f64, direction: f64,
) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            // Remove any existing gradient background view (tagged 9999)
            let subviews: Retained<AnyObject> = objc2::msg_send![&*view, subviews];
            let count: usize = objc2::msg_send![&*subviews, count];
            let mut i = count;
            while i > 0 {
                i -= 1;
                let sub: *mut AnyObject = objc2::msg_send![&*subviews, objectAtIndex: i];
                let tag: isize = objc2::msg_send![sub, tag];
                if tag == 9999 {
                    let _: () = objc2::msg_send![sub, removeFromSuperview];
                }
            }

            // Create PerryGradientBGView (overrides layoutSubviews to resize sublayers)
            let bg_cls = gradient_bg_class();
            let bg_view: *mut AnyObject = objc2::msg_send![bg_cls, new];
            let _: () = objc2::msg_send![bg_view, setTag: 9999isize];
            let _: () = objc2::msg_send![bg_view, setTranslatesAutoresizingMaskIntoConstraints: false];
            let _: () = objc2::msg_send![bg_view, setUserInteractionEnabled: false];

            // Insert as subview at index 0 (behind arranged subviews / content)
            let _: () = objc2::msg_send![&*view, insertSubview: bg_view, atIndex: 0isize];

            // Pin bg_view to parent edges via Auto Layout
            let bg_leading: *mut AnyObject = objc2::msg_send![bg_view, leadingAnchor];
            let bg_trailing: *mut AnyObject = objc2::msg_send![bg_view, trailingAnchor];
            let bg_top: *mut AnyObject = objc2::msg_send![bg_view, topAnchor];
            let bg_bottom: *mut AnyObject = objc2::msg_send![bg_view, bottomAnchor];

            let p_leading: *mut AnyObject = objc2::msg_send![&*view, leadingAnchor];
            let p_trailing: *mut AnyObject = objc2::msg_send![&*view, trailingAnchor];
            let p_top: *mut AnyObject = objc2::msg_send![&*view, topAnchor];
            let p_bottom: *mut AnyObject = objc2::msg_send![&*view, bottomAnchor];

            let c1: *mut AnyObject = objc2::msg_send![bg_leading, constraintEqualToAnchor: p_leading];
            let c2: *mut AnyObject = objc2::msg_send![bg_trailing, constraintEqualToAnchor: p_trailing];
            let c3: *mut AnyObject = objc2::msg_send![bg_top, constraintEqualToAnchor: p_top];
            let c4: *mut AnyObject = objc2::msg_send![bg_bottom, constraintEqualToAnchor: p_bottom];

            let _: () = objc2::msg_send![c1, setActive: true];
            let _: () = objc2::msg_send![c2, setActive: true];
            let _: () = objc2::msg_send![c3, setActive: true];
            let _: () = objc2::msg_send![c4, setActive: true];

            // Create CAGradientLayer on the background view
            let bg_layer: *mut AnyObject = objc2::msg_send![bg_view, layer];
            let gradient_cls = AnyClass::get(c"CAGradientLayer")
                .expect("CAGradientLayer class not found");
            let gradient: *mut AnyObject = objc2::msg_send![gradient_cls, layer];

            // Create colors
            let color1 = create_cg_color(r1, g1, b1, a1);
            let color2 = create_cg_color(r2, g2, b2, a2);

            let colors: Retained<AnyObject> = {
                let arr_cls = AnyClass::get(c"NSMutableArray").unwrap();
                let arr: *mut AnyObject = objc2::msg_send![arr_cls, arrayWithCapacity: 2usize];
                let _: () = objc2::msg_send![arr, addObject: color1 as *mut AnyObject];
                let _: () = objc2::msg_send![arr, addObject: color2 as *mut AnyObject];
                Retained::retain(arr).unwrap()
            };
            let _: () = objc2::msg_send![gradient, setColors: &*colors];
            CGColorRelease(color1);
            CGColorRelease(color2);

            // Set direction
            if direction < 0.5 {
                let start = objc2_core_foundation::CGPoint::new(0.5, 0.0);
                let end = objc2_core_foundation::CGPoint::new(0.5, 1.0);
                let _: () = objc2::msg_send![gradient, setStartPoint: start];
                let _: () = objc2::msg_send![gradient, setEndPoint: end];
            } else {
                let start = objc2_core_foundation::CGPoint::new(0.0, 0.5);
                let end = objc2_core_foundation::CGPoint::new(1.0, 0.5);
                let _: () = objc2::msg_send![gradient, setStartPoint: start];
                let _: () = objc2::msg_send![gradient, setEndPoint: end];
            }

            // Add gradient to the background view's layer
            let _: () = objc2::msg_send![bg_layer, addSublayer: gradient];
        }
    }
}

/// Set corner radius on any widget via its layer.
pub fn set_corner_radius(handle: i64, radius: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let _: () = objc2::msg_send![layer, setCornerRadius: radius];
            }
            let _: () = objc2::msg_send![&*view, setClipsToBounds: true];
        }
    }
}

/// Set a fixed width constraint on a widget.
pub fn set_width(handle: i64, width: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let width_anchor: Retained<AnyObject> = objc2::msg_send![&*view, widthAnchor];
            let constraint: Retained<AnyObject> = objc2::msg_send![
                &*width_anchor, constraintEqualToConstant: width
            ];
            let _: () = objc2::msg_send![&*constraint, setActive: true];
        }
    }
}

/// Set a fixed height constraint on a widget.
/// Idempotent: deactivates any previous height constraint before creating a new one.
pub fn set_height(handle: i64, height: f64) {
    if let Some(view) = get_widget(handle) {
        // Deactivate old height constraint if any
        HEIGHT_CONSTRAINTS.with(|hc| {
            let mut map = hc.borrow_mut();
            if let Some(old) = map.remove(&handle) {
                unsafe {
                    let _: () = objc2::msg_send![&*old, setActive: false];
                }
            }
        });
        unsafe {
            let height_anchor: Retained<AnyObject> = objc2::msg_send![&*view, heightAnchor];
            let constraint: Retained<AnyObject> = objc2::msg_send![
                &*height_anchor, constraintEqualToConstant: height
            ];
            let _: () = objc2::msg_send![&*constraint, setActive: true];
            HEIGHT_CONSTRAINTS.with(|hc| {
                hc.borrow_mut().insert(handle, constraint);
            });
        }
    }
}

/// Remove a child view from a parent view.
/// If the parent is a UIStackView, removes from arranged subviews first.
pub fn remove_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let is_stack = if let Some(cls) = AnyClass::get(c"UIStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &UIStackView = unsafe { &*(Retained::as_ptr(&parent) as *const UIStackView) };
            unsafe {
                let _: () = objc2::msg_send![stack, removeArrangedSubview: &*child];
            }
        }
        child.removeFromSuperview();
    }
}

/// Reorder a child widget within a parent (UIStackView) by index.
pub fn reorder_child(parent_handle: i64, from_index: i64, to_index: i64) {
    if let Some(parent) = get_widget(parent_handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"UIStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let stack: &UIStackView = unsafe { &*(Retained::as_ptr(&parent) as *const UIStackView) };
            let subviews = stack.arrangedSubviews();
            let from = from_index as usize;
            if from < subviews.len() {
                // Get child at index via objectAtIndex:
                unsafe {
                    let child: *mut AnyObject = objc2::msg_send![&*subviews, objectAtIndex: from];
                    if !child.is_null() {
                        let _: () = objc2::msg_send![stack, removeArrangedSubview: child];
                        let _: () = objc2::msg_send![stack, insertArrangedSubview: child, atIndex: to_index as usize];
                    }
                }
            }
        }
    }
}

/// Pin a child view's width to match its containing UIStackView.
pub fn match_parent_width(child_handle: i64) {
    if let Some(child) = get_widget(child_handle) {
        unsafe {
            let stack_cls = AnyClass::get(c"UIStackView");
            if stack_cls.is_none() { return; }
            let stack_cls = stack_cls.unwrap();
            let mut sv: *const UIView = objc2::msg_send![&*child, superview];
            let mut found_stack: *const UIView = std::ptr::null();
            let mut depth = 0;
            while !sv.is_null() && depth < 10 {
                let is_stack: bool = objc2::msg_send![sv, isKindOfClass: stack_cls];
                if is_stack {
                    found_stack = sv;
                    break;
                }
                sv = objc2::msg_send![sv, superview];
                depth += 1;
            }
            if found_stack.is_null() { return; }
            let child_width: Retained<AnyObject> = objc2::msg_send![&*child, widthAnchor];
            // Use layoutMarginsGuide so the constraint respects the parent's padding/edge insets
            let margins_guide: Retained<AnyObject> = objc2::msg_send![found_stack, layoutMarginsGuide];
            let stack_width: Retained<AnyObject> = objc2::msg_send![&*margins_guide, widthAnchor];
            let c: Retained<AnyObject> = objc2::msg_send![&*child_width, constraintEqualToAnchor: &*stack_width];
            let _: () = objc2::msg_send![&*c, setActive: true];
        }
    }
}

/// Pin a child view's top and bottom anchors to its superview, forcing it to
/// fill the parent's height.
pub fn match_parent_height(child_handle: i64) {
    if let Some(child) = get_widget(child_handle) {
        unsafe {
            let superview_ptr: *const UIView = objc2::msg_send![&*child, superview];
            if superview_ptr.is_null() { return; }
            let child_top: Retained<AnyObject> = objc2::msg_send![&*child, topAnchor];
            let child_bottom: Retained<AnyObject> = objc2::msg_send![&*child, bottomAnchor];
            let parent_top: Retained<AnyObject> = objc2::msg_send![superview_ptr, topAnchor];
            let parent_bottom: Retained<AnyObject> = objc2::msg_send![superview_ptr, bottomAnchor];
            let top_c: Retained<AnyObject> = objc2::msg_send![&*child_top, constraintEqualToAnchor: &*parent_top];
            let bot_c: Retained<AnyObject> = objc2::msg_send![&*child_bottom, constraintEqualToAnchor: &*parent_bottom];
            let _: () = objc2::msg_send![&*top_c, setActive: true];
            let _: () = objc2::msg_send![&*bot_c, setActive: true];
        }
    }
}

/// Set detachesHiddenViews equivalent — no-op on iOS.
/// UIStackView on iOS automatically adjusts for hidden views.
pub fn set_detaches_hidden_views(_handle: i64, _detaches: bool) {
    // No-op: UIStackView on iOS doesn't have this property.
    // Hidden arranged subviews are automatically excluded from layout.
}

/// Set the content hugging priority for both axes.
pub fn set_hugging_priority(handle: i64, priority: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let p = priority as f32;
            // UILayoutConstraintAxis: 0 = Horizontal, 1 = Vertical
            let _: () = objc2::msg_send![&*view, setContentHuggingPriority: p, forAxis: 0isize];
            let _: () = objc2::msg_send![&*view, setContentHuggingPriority: p, forAxis: 1isize];
        }
    }
}

// =============================================================================
// Cross-cutting: Enabled, Hover, DoubleClick, Animations, Tooltip, ControlSize
// =============================================================================

use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

thread_local! {
    static DOUBLE_CLICK_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}

/// Set the enabled state of any UIControl-based widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setEnabled: enabled];
        }
    }
}

/// Set a tooltip (no-op on iOS).
pub fn set_tooltip(_handle: i64, _text: &str) {}

/// Set control size variant (no-op on iOS).
pub fn set_control_size(_handle: i64, _size: i64) {}

/// Animate the opacity of a widget. `duration_secs` is in seconds.
pub fn animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            let anim_cls = AnyClass::get(c"CABasicAnimation").unwrap();
            let key = objc2_foundation::NSString::from_str("opacity");
            let anim: *mut AnyObject = objc2::msg_send![anim_cls, animationWithKeyPath: &*key];
            let target_ns: Retained<AnyObject> = objc2::msg_send![
                AnyClass::get(c"NSNumber").unwrap(), numberWithDouble: target
            ];
            let _: () = objc2::msg_send![anim, setToValue: &*target_ns];
            let _: () = objc2::msg_send![anim, setDuration: duration_secs];
            let _: () = objc2::msg_send![anim, setFillMode:
                &*objc2_foundation::NSString::from_str("forwards")];
            let _: () = objc2::msg_send![anim, setRemovedOnCompletion: false];
            let _: () = objc2::msg_send![layer, addAnimation: anim, forKey: &*key];
            let _: () = objc2::msg_send![&*view, setAlpha: target];
        }
    }
}

/// Animate the position of a widget by delta. `duration_secs` is in seconds.
pub fn animate_position(handle: i64, dx: f64, dy: f64, duration_secs: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let frame: objc2_core_foundation::CGRect = objc2::msg_send![&*view, frame];
            let new_frame = objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(frame.origin.x + dx, frame.origin.y + dy),
                frame.size,
            );
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            let anim_cls = AnyClass::get(c"CABasicAnimation").unwrap();
            let key = objc2_foundation::NSString::from_str("position");
            let anim: *mut AnyObject = objc2::msg_send![anim_cls, animationWithKeyPath: &*key];
            let _: () = objc2::msg_send![anim, setDuration: duration_secs];
            let _: () = objc2::msg_send![layer, addAnimation: anim, forKey: &*key];
            let _: () = objc2::msg_send![&*view, setFrame: new_frame];
        }
    }
}

/// Set on-hover (no-op on iOS).
pub fn set_on_hover(_handle: i64, _callback: f64) {}

/// Set a single-tap handler for any widget.
pub fn set_on_click(handle: i64, callback: f64) {
    button::set_on_tap(handle, callback);
}

/// Set a double-tap handler.
pub fn set_on_double_click(handle: i64, callback: f64) {
    DOUBLE_CLICK_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(handle, callback);
    });
    if let Some(view) = get_widget(handle) {
        unsafe {
            let gr_cls = AnyClass::get(c"UITapGestureRecognizer").unwrap();
            let recognizer: *mut AnyObject = objc2::msg_send![gr_cls, alloc];
            let recognizer: *mut AnyObject = objc2::msg_send![recognizer, init];
            let _: () = objc2::msg_send![recognizer, setNumberOfTapsRequired: 2i64];
            let _: () = objc2::msg_send![&*view, addGestureRecognizer: recognizer];
        }
    }
}
