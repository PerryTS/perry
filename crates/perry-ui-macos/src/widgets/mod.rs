pub mod alert;
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
pub mod qrcode;
pub mod scrollview;
pub mod securefield;
pub mod sheet;
pub mod slider;
pub mod spacer;
pub mod table;
pub mod text;
pub mod text_registry;
pub mod textarea;
pub mod textfield;
pub mod toast;
pub mod toggle;
pub mod toolbar;
pub mod vstack;
pub mod zstack;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{NSStackView, NSView};
use objc2_foundation::NSObjectProtocol;
use std::cell::RefCell;

thread_local! {
    /// Map from widget handle (1-based) to NSView
    static WIDGETS: RefCell<Vec<Retained<NSView>>> = const { RefCell::new(Vec::new()) };
    /// Stored width constraints per widget handle, so set_width can update instead of duplicate.
    static WIDTH_CONSTRAINTS: RefCell<std::collections::HashMap<i64, Retained<AnyObject>>> = RefCell::new(std::collections::HashMap::new());
    /// Stored height constraints per widget handle, so set_height can update instead of duplicate.
    static HEIGHT_CONSTRAINTS: RefCell<std::collections::HashMap<i64, Retained<AnyObject>>> = RefCell::new(std::collections::HashMap::new());
    /// Parent tracking: child_handle -> (parent_handle, insertion_index)
    /// Used by set_hidden to re-insert views that NSStackView detached.
    static PARENT_MAP: RefCell<std::collections::HashMap<i64, (i64, usize)>> = RefCell::new(std::collections::HashMap::new());
    /// Background color cache: handle -> (r, g, b, a)
    /// Used to re-apply bg color after NSStackView detach/re-attach.
    static BG_COLOR_MAP: RefCell<std::collections::HashMap<i64, (f64, f64, f64, f64)>> = RefCell::new(std::collections::HashMap::new());
}

/// Store an NSView and return its handle (1-based i64).
pub fn register_widget(view: Retained<NSView>) -> i64 {
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
    if buf.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
        *out_len = len;
    }
    buf
}

/// Read the current value of a widget by handle (text field → string, slider → number, button → state).
#[cfg(feature = "geisterhand")]
#[no_mangle]
pub extern "C" fn perry_ui_read_widget_value(handle: i64, out_len: *mut usize) -> *mut u8 {
    unsafe {
        *out_len = 0;
    }
    if let Some(view) = get_widget(handle) {
        unsafe {
            if let Some(tf_cls) = objc2::runtime::AnyClass::get(c"NSTextField") {
                let is_tf: bool = msg_send![&*view, isKindOfClass: tf_cls];
                if is_tf {
                    let val: *const objc2::runtime::AnyObject = msg_send![&*view, stringValue];
                    if !val.is_null() {
                        let ns: &objc2_foundation::NSString =
                            &*(val as *const objc2_foundation::NSString);
                        return alloc_string_result(&ns.to_string(), out_len);
                    }
                }
            }
            if let Some(slider_cls) = objc2::runtime::AnyClass::get(c"NSSlider") {
                let is_slider: bool = msg_send![&*view, isKindOfClass: slider_cls];
                if is_slider {
                    let val: f64 = msg_send![&*view, doubleValue];
                    return alloc_string_result(&format!("{}", val), out_len);
                }
            }
            if let Some(btn_cls) = objc2::runtime::AnyClass::get(c"NSButton") {
                let is_btn: bool = msg_send![&*view, isKindOfClass: btn_cls];
                if is_btn {
                    let state: isize = msg_send![&*view, state];
                    return alloc_string_result(if state == 1 { "true" } else { "false" }, out_len);
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
                let hidden: bool = msg_send![&**view, isHidden];
                let visible = !hidden;
                let frame: CGRect = msg_send![&**view, frame];
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
        unsafe {
            *out_len = 0;
        }
        return std::ptr::null_mut();
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
        *out_len = len;
    }
    buf
}

/// Register an external NSView (e.g. from a native library) into the widget system.
/// The raw pointer is retained and assigned a handle usable with widgetAddChild etc.
pub fn register_external_nsview(nsview_ptr: i64) -> i64 {
    if nsview_ptr == 0 {
        eprintln!("register_external_nsview: null pointer passed, returning 0");
        return 0;
    }
    match unsafe { Retained::retain(nsview_ptr as *mut NSView) } {
        Some(nsview) => {
            // Disable autoresizing mask constraints so the view can be sized
            // by NSStackView layout instead of being fixed at its initial frame size.
            unsafe {
                let _: () =
                    objc2::msg_send![&*nsview, setTranslatesAutoresizingMaskIntoConstraints: false];
            }
            // Set low content hugging so it stretches in both axes.
            unsafe {
                let _: () =
                    msg_send![&*nsview, setContentHuggingPriority: 1.0f32 forOrientation: 0i64]; // horizontal
                let _: () =
                    msg_send![&*nsview, setContentHuggingPriority: 1.0f32 forOrientation: 1i64];
                // vertical
            }
            // Clip to bounds — prevent the view from drawing outside its frame
            // (e.g. editor content bleeding behind tab bar/breadcrumb in VStack)
            unsafe {
                let _: () = objc2::msg_send![&*nsview, setWantsLayer: true];
                let layer: *mut AnyObject = objc2::msg_send![&*nsview, layer];
                if !layer.is_null() {
                    let _: () = objc2::msg_send![layer, setMasksToBounds: true];
                    // Ensure the layer renders at Retina resolution.
                    // External NSViews (e.g. hone-editor) may set this during
                    // init, but embedding into an NSStackView can reset it.
                    let screen: *mut AnyObject = objc2::msg_send![
                        objc2::runtime::AnyClass::get(c"NSScreen").unwrap(),
                        mainScreen
                    ];
                    if !screen.is_null() {
                        let scale: f64 = objc2::msg_send![screen, backingScaleFactor];
                        let _: () = objc2::msg_send![layer, setContentsScale: scale];
                    }
                }
            }
            register_widget(nsview)
        }
        None => {
            eprintln!(
                "register_external_nsview: failed to retain NSView at {:#x}",
                nsview_ptr
            );
            0
        }
    }
}

/// Retrieve the NSView for a given handle.
pub fn get_widget(handle: i64) -> Option<Retained<NSView>> {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        let idx = (handle - 1) as usize;
        widgets.get(idx).cloned()
    })
}

/// Set the hidden state of a widget.
/// When unhiding a view that NSStackView detached (no superview), re-inserts it
/// into its parent NSStackView at the original position.
pub fn set_hidden(handle: i64, hidden: bool) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setHidden: hidden];
        }

        if !hidden {
            // Check if the view got orphaned (no superview) after unhiding.
            // This happens when NSStackView's detachesHiddenViews=true removed it.
            let needs_reattach = unsafe {
                let sv: *const NSView = objc2::msg_send![&*view, superview];
                sv.is_null()
            };

            if needs_reattach {
                let parent_info = PARENT_MAP.with(|m| m.borrow().get(&handle).copied());
                if let Some((parent_handle, index)) = parent_info {
                    if let Some(parent) = get_widget(parent_handle) {
                        let is_stack = AnyClass::get(c"NSStackView")
                            .map(|cls| parent.isKindOfClass(cls))
                            .unwrap_or(false);
                        if is_stack {
                            let stack: &NSStackView =
                                unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
                            let count = stack.arrangedSubviews().len();
                            let insert_idx = index.min(count);
                            unsafe {
                                let _: () = objc2::msg_send![
                                    stack, insertArrangedSubview: &*view, atIndex: insert_idx
                                ];
                            }
                        }
                    }
                }
            }

            // Re-apply cached background color (NSStackView detach strips CALayer bg)
            let cached_bg = BG_COLOR_MAP.with(|m| m.borrow().get(&handle).copied());
            if let Some((r, g, b, a)) = cached_bg {
                apply_background_color(handle, r, g, b, a);
            }
        }
    }
}

/// Set detachesHiddenViews on an NSStackView.
/// When false, hidden views still participate in layout (occupy space but are invisible).
pub fn set_detaches_hidden_views(handle: i64, detaches: bool) {
    if let Some(view) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            unsafe {
                let _: () = msg_send![&*view, setDetachesHiddenViews: detaches];
            }
        }
    }
}

/// Set distribution on an NSStackView.
/// 0 = Fill, 1 = FillEqually, 2 = FillProportionally,
/// 3 = EqualSpacing, 4 = EqualCentering, -1 = GravityAreas.
pub fn set_distribution(handle: i64, distribution: i64) {
    if let Some(view) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            unsafe {
                let _: () = msg_send![&*view, setDistribution: distribution];
            }
        }
    }
}

/// Set alignment on an NSStackView.
/// For vertical stacks: Leading=5, CenterX=9, Width=7.
/// For horizontal stacks: CenterY=12, Top=3, Bottom=4.
pub fn set_alignment(handle: i64, alignment: i64) {
    if let Some(view) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            unsafe {
                let _: () = msg_send![&*view, setAlignment: alignment];
            }
        }
    }
}

/// Find the widget handle for an NSView pointer by scanning the WIDGETS vec.
fn find_handle_for_view(view: &NSView) -> Option<i64> {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        for (idx, widget) in widgets.iter().enumerate() {
            if std::ptr::eq(Retained::as_ptr(widget), view as *const NSView) {
                return Some((idx + 1) as i64);
            }
        }
        None
    })
}

/// Recursively collect widget handles for all descendants of a view.
fn collect_subtree_handles(view: &NSView) -> Vec<i64> {
    let mut handles = Vec::new();
    let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
        view.isKindOfClass(cls)
    } else {
        false
    };
    if is_stack {
        let stack: &NSStackView = unsafe { &*(view as *const NSView as *const NSStackView) };
        let subviews = stack.arrangedSubviews();
        let count = subviews.len();
        for i in 0..count {
            let sv: *const NSView = unsafe { objc2::msg_send![&subviews, objectAtIndex: i] };
            if !sv.is_null() {
                let sv_ref = unsafe { &*sv };
                if let Some(h) = find_handle_for_view(sv_ref) {
                    handles.push(h);
                }
                // Recurse into nested stacks
                handles.extend(collect_subtree_handles(sv_ref));
            }
        }
    }
    handles
}

/// Clean up metadata maps for a list of widget handles.
fn cleanup_widget_maps(handles: &[i64]) {
    for handle in handles {
        PARENT_MAP.with(|m| {
            m.borrow_mut().remove(handle);
        });
        BG_COLOR_MAP.with(|m| {
            m.borrow_mut().remove(handle);
        });
        WIDTH_CONSTRAINTS.with(|wc| {
            if let Some(old) = wc.borrow_mut().remove(handle) {
                unsafe {
                    let _: () = objc2::msg_send![&*old, setActive: false];
                }
            }
        });
        HEIGHT_CONSTRAINTS.with(|hc| {
            if let Some(old) = hc.borrow_mut().remove(handle) {
                unsafe {
                    let _: () = objc2::msg_send![&*old, setActive: false];
                }
            }
        });
    }
}

/// Remove all arranged subviews from a container (NSStackView).
/// Safe implementation: snapshots subviews before mutation, deactivates
/// constraints in batch, removes in reverse order, and cleans up metadata maps.
pub fn clear_children(handle: i64) {
    if let Some(parent) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let stack: &NSStackView =
                unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };

            // Phase 1: Snapshot subviews into a Vec (avoid iterator invalidation)
            let subviews = stack.arrangedSubviews();
            let count = subviews.len();
            let mut views: Vec<Retained<NSView>> = Vec::with_capacity(count);
            for i in 0..count {
                let sv: *const NSView = unsafe { objc2::msg_send![&subviews, objectAtIndex: i] };
                if !sv.is_null() {
                    if let Some(retained) = unsafe { Retained::retain(sv as *mut NSView) } {
                        views.push(retained);
                    }
                }
            }

            // Phase 2: Collect all handles (including nested descendants)
            let mut all_handles = Vec::new();
            for sv in &views {
                if let Some(h) = find_handle_for_view(sv) {
                    all_handles.push(h);
                }
                all_handles.extend(collect_subtree_handles(sv));
            }

            // Phase 3: Batch deactivate constraints on all views
            for sv in &views {
                unsafe {
                    let constraints: Retained<AnyObject> = objc2::msg_send![&**sv, constraints];
                    let c_count: usize = objc2::msg_send![&*constraints, count];
                    for i in 0..c_count {
                        let c: *mut AnyObject = objc2::msg_send![&*constraints, objectAtIndex: i];
                        if !c.is_null() {
                            let _: () = objc2::msg_send![c, setActive: false];
                        }
                    }
                }
            }

            // Phase 4: Remove views in reverse order
            for sv in views.iter().rev() {
                stack.removeArrangedSubview(sv);
                sv.removeFromSuperview();
            }

            // Phase 5: Clean up metadata maps for all removed widgets
            cleanup_widget_maps(&all_handles);
        }
    }
}

/// Add a child view to a parent view at a specific index.
pub fn add_child_at(parent_handle: i64, child_handle: i64, index: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &NSStackView =
                unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
            // Use addView:inGravity: with top/leading gravity for consistent packing
            unsafe {
                let _: () = objc2::msg_send![stack, addView: &*child, inGravity: 1i64];
            }
            // Track parent-child for re-attachment after hide/show
            PARENT_MAP.with(|m| {
                m.borrow_mut()
                    .insert(child_handle, (parent_handle, index as usize));
            });
        } else if zstack::is_zstack(parent_handle) {
            zstack::add_child(parent_handle, child_handle);
        } else {
            parent.addSubview(&child);
        }
    }
}

/// Add a child view to a parent view.
/// If the parent is an NSStackView, uses addArrangedSubview for proper layout.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        // Check if parent is an NSStackView
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            // Safety: we verified the type with isKindOfClass
            let stack: &NSStackView =
                unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
            let index = stack.arrangedSubviews().len();
            // Use addView:inGravity: with Top/Leading gravity (1) so children
            // pack tightly from the top (VStack) or leading edge (HStack)
            // instead of defaulting to center gravity area.
            unsafe {
                let _: () = objc2::msg_send![stack, addView: &*child, inGravity: 1i64];
            }
            // Track parent-child for re-attachment after hide/show
            PARENT_MAP.with(|m| {
                m.borrow_mut().insert(child_handle, (parent_handle, index));
            });
        } else if zstack::is_zstack(parent_handle) {
            zstack::add_child(parent_handle, child_handle);
        } else {
            parent.addSubview(&child);
        }
    }
}

/// Remove a child view from a parent view.
/// If the parent is an NSStackView, removes from arranged subviews first.
/// Also cleans up metadata maps for the removed child and its descendants.
pub fn remove_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        // Collect handles for cleanup before removal
        let mut handles_to_clean = vec![child_handle];
        handles_to_clean.extend(collect_subtree_handles(&child));

        if is_stack {
            let stack: &NSStackView =
                unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
            stack.removeArrangedSubview(&child);
        }
        child.removeFromSuperview();

        // Clean up metadata maps
        cleanup_widget_maps(&handles_to_clean);
    }
}

/// Add a child as a floating overlay (plain addSubview, not arranged).
/// The overlay floats on top of the parent's stack layout.
/// Caller must set frame via widgetSetOverlayFrame(child, x, y, w, h).
pub fn add_overlay(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        // Always use plain addSubview (not addArrangedSubview) so it floats on top
        parent.addSubview(&child);
    }
}

/// Set the frame (position + size) of an overlay child.
/// x/y are relative to the parent's coordinate system.
pub fn set_overlay_frame(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            view.setTranslatesAutoresizingMaskIntoConstraints(true);
            let frame = objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(x, y),
                objc2_core_foundation::CGSize::new(w, h),
            );
            view.setFrame(frame);
        }
    }
}

/// Reorder a child within an NSStackView by moving from one index to another.
pub fn reorder_child(parent_handle: i64, from_index: i64, to_index: i64) {
    if let Some(parent) = get_widget(parent_handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &NSStackView =
                unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
            let subviews = stack.arrangedSubviews();
            let count = subviews.len();
            let fi = from_index as usize;
            let ti = to_index as usize;
            if fi < count && ti < count {
                let child: *const NSView =
                    unsafe { objc2::msg_send![&subviews, objectAtIndex: fi] };
                let child_ref: &NSView = unsafe { &*child };
                stack.removeArrangedSubview(child_ref);
                unsafe {
                    let _: () =
                        objc2::msg_send![stack, insertArrangedSubview: child_ref, atIndex: ti];
                }
            }
        }
    }
}

// =============================================================================
// Widget Styling (Background, Gradient, Corner Radius)
// =============================================================================

use std::ffi::c_void;

type CGFloat = f64;

extern "C" {
    fn CGColorCreateGenericRGB(r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) -> *mut c_void;
    fn CGColorRelease(color: *mut c_void);
}

/// Set a solid background color on any widget via its layer.
/// Also caches the color so it can be re-applied after NSStackView detach/re-attach.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    // Cache the color for re-application after hide/show
    BG_COLOR_MAP.with(|m| {
        m.borrow_mut().insert(handle, (r, g, b, a));
    });
    apply_background_color(handle, r, g, b, a);
}

fn apply_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let cg_color = CGColorCreateGenericRGB(r, g, b, a);
                let _: () = objc2::msg_send![layer, setBackgroundColor: cg_color];
                CGColorRelease(cg_color);
            }
        }
    }
}

/// Set a linear gradient background on any widget via CAGradientLayer.
pub fn set_background_gradient(
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
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if layer.is_null() {
                return;
            }

            // Remove any existing gradient sublayer (tagged by name "PerryGradient")
            let sublayers: *mut AnyObject = objc2::msg_send![layer, sublayers];
            if !sublayers.is_null() {
                let count: usize = objc2::msg_send![sublayers, count];
                // Iterate backwards to safely remove
                let mut i = count;
                while i > 0 {
                    i -= 1;
                    let sub: *mut AnyObject = objc2::msg_send![sublayers, objectAtIndex: i];
                    let name: *mut AnyObject = objc2::msg_send![sub, name];
                    if !name.is_null() {
                        let is_ours: bool = objc2::msg_send![name, isEqualToString:
                            &*objc2_foundation::NSString::from_str("PerryGradient")];
                        if is_ours {
                            let _: () = objc2::msg_send![sub, removeFromSuperlayer];
                        }
                    }
                }
            }

            // Create CAGradientLayer
            let gradient_cls =
                AnyClass::get(c"CAGradientLayer").expect("CAGradientLayer class not found");
            let gradient: *mut AnyObject = objc2::msg_send![gradient_cls, layer];

            // Set name for later removal
            let name = objc2_foundation::NSString::from_str("PerryGradient");
            let _: () = objc2::msg_send![gradient, setName: &*name];

            // Set frame to match layer bounds
            let bounds: objc2_core_foundation::CGRect = objc2::msg_send![layer, bounds];
            let _: () = objc2::msg_send![gradient, setFrame: bounds];

            // Create colors array
            let color1 = CGColorCreateGenericRGB(r1, g1, b1, a1);
            let color2 = CGColorCreateGenericRGB(r2, g2, b2, a2);

            // Wrap in NSArray via obj-c id
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
                // Vertical: top to bottom
                let start = objc2_core_foundation::CGPoint::new(0.5, 0.0);
                let end = objc2_core_foundation::CGPoint::new(0.5, 1.0);
                let _: () = objc2::msg_send![gradient, setStartPoint: start];
                let _: () = objc2::msg_send![gradient, setEndPoint: end];
            } else {
                // Horizontal: left to right
                let start = objc2_core_foundation::CGPoint::new(0.0, 0.5);
                let end = objc2_core_foundation::CGPoint::new(1.0, 0.5);
                let _: () = objc2::msg_send![gradient, setStartPoint: start];
                let _: () = objc2::msg_send![gradient, setEndPoint: end];
            }

            // Insert at index 0 (behind other sublayers)
            let _: () = objc2::msg_send![layer, insertSublayer: gradient, atIndex: 0u32];

            // Auto-resize gradient with the layer
            let mask: u32 = (1 << 1) | (1 << 4); // kCALayerWidthSizable | kCALayerHeightSizable
            let _: () = objc2::msg_send![gradient, setAutoresizingMask: mask];
        }
    }
}

/// Set the border color on any widget via its layer.
pub fn set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let cg_color = CGColorCreateGenericRGB(r, g, b, a);
                let _: () = objc2::msg_send![layer, setBorderColor: cg_color];
                CGColorRelease(cg_color);
            }
        }
    }
}

/// Set the border width on any widget via its layer.
pub fn set_border_width(handle: i64, width: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let _: () = objc2::msg_send![layer, setBorderWidth: width];
            }
        }
    }
}

/// Set a drop shadow on any widget via its layer.
///
/// `(r, g, b, a)` is the shadow color (0.0–1.0); `a` is folded into
/// `setShadowOpacity:` because CALayer treats `shadowColor` as opaque
/// regardless of the CGColor's alpha. `blur` is `setShadowRadius:`.
/// `(offset_x, offset_y)` is `setShadowOffset:` — note CALayer uses
/// CGSize{width = x, height = y} where positive y is **downward** on
/// macOS (matches HTML's `box-shadow: x y blur color`). Setting blur
/// to 0 with a non-zero offset gives a hard shadow; setting all of
/// blur + offset_x + offset_y to 0 effectively clears the shadow.
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
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                // Color: opaque CGColor; alpha rides on shadowOpacity.
                let cg_color = CGColorCreateGenericRGB(r, g, b, 1.0);
                let _: () = objc2::msg_send![layer, setShadowColor: cg_color];
                CGColorRelease(cg_color);
                let _: () = objc2::msg_send![layer, setShadowOpacity: a as f32];
                let _: () = objc2::msg_send![layer, setShadowRadius: blur];
                // CGSize is {f64 width, f64 height}; objc2_core_foundation
                // already provides this layout-compatible struct.
                let offset = objc2_core_foundation::CGSize::new(offset_x, offset_y);
                let _: () = objc2::msg_send![layer, setShadowOffset: offset];
                // CALayer shadows are clipped by `masksToBounds`. iOS
                // sets this in its mirror impl; the macOS version was
                // missing it — the bug visible in
                // `docs/examples/ui/styling/visual_test.ts` section 4
                // before this fix landed (FFI args correct per the IR
                // dump but no visible shadow).
                let _: () = objc2::msg_send![layer, setMasksToBounds: false];
            }
        }
    }
}

/// Set edge insets (internal padding) on an NSStackView widget.
/// No-op for non-stack widgets.
pub fn set_edge_insets(handle: i64, top: f64, left: f64, bottom: f64, right: f64) {
    if let Some(view) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let stack: &NSStackView = unsafe { &*(Retained::as_ptr(&view) as *const NSStackView) };
            unsafe {
                stack.setEdgeInsets(objc2_foundation::NSEdgeInsets {
                    top,
                    left,
                    bottom,
                    right,
                });
            }
        }
    }
}

/// Set the view's alpha (opacity) in [0.0, 1.0].
pub fn set_opacity(handle: i64, alpha: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setAlphaValue: alpha];
        }
    }
}

/// Set corner radius on any widget via its layer.
pub fn set_corner_radius(handle: i64, radius: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let _: () = objc2::msg_send![layer, setCornerRadius: radius];
                let _: () = objc2::msg_send![layer, setMasksToBounds: true];
            }
        }
    }
}

/// Set a fixed width constraint on a widget.
/// Idempotent: deactivates any previous width constraint before creating a new one.
pub fn set_width(handle: i64, width: f64) {
    if let Some(view) = get_widget(handle) {
        // Deactivate old width constraint if any
        WIDTH_CONSTRAINTS.with(|wc| {
            let mut map = wc.borrow_mut();
            if let Some(old) = map.remove(&handle) {
                unsafe {
                    let _: () = msg_send![&*old, setActive: false];
                }
            }
        });
        unsafe {
            let width_anchor: Retained<AnyObject> = msg_send![&*view, widthAnchor];
            let constraint: Retained<AnyObject> = msg_send![
                &*width_anchor, constraintEqualToConstant: width
            ];
            let _: () = msg_send![&*constraint, setActive: true];
            // Store for future updates
            WIDTH_CONSTRAINTS.with(|wc| {
                wc.borrow_mut().insert(handle, constraint);
            });
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
                    let _: () = msg_send![&*old, setActive: false];
                }
            }
        });
        unsafe {
            let height_anchor: Retained<AnyObject> = msg_send![&*view, heightAnchor];
            let constraint: Retained<AnyObject> = msg_send![
                &*height_anchor, constraintEqualToConstant: height
            ];
            let _: () = msg_send![&*constraint, setActive: true];
            // Store for future updates
            HEIGHT_CONSTRAINTS.with(|hc| {
                hc.borrow_mut().insert(handle, constraint);
            });
        }
    }
}

/// Set the content hugging priority for horizontal orientation.
/// Low values (1-250) mean the view is willing to stretch.
/// High values (750-1000) mean the view resists stretching.
pub fn set_hugging_priority(handle: i64, priority: f64) {
    if let Some(view) = get_widget(handle) {
        use objc2_app_kit::NSLayoutConstraintOrientation;
        view.setContentHuggingPriority_forOrientation(
            priority as f32,
            NSLayoutConstraintOrientation::Horizontal,
        );
        view.setContentHuggingPriority_forOrientation(
            priority as f32,
            NSLayoutConstraintOrientation::Vertical,
        );
    }
}

/// Pin a child view's width to match its containing NSStackView.
/// Walks the superview chain to find the nearest NSStackView, then pins
/// the child's widthAnchor to that stack's widthAnchor. Useful for VStack
/// children (especially embedded NSViews) that should stretch horizontally.
pub fn match_stack_width(child_handle: i64) {
    if let Some(child) = get_widget(child_handle) {
        unsafe {
            // Walk up the superview chain to find the NSStackView.
            // NSStackView may wrap arranged subviews in intermediate views.
            let stack_cls = AnyClass::get(c"NSStackView");
            if stack_cls.is_none() {
                eprintln!("match_parent_width: NSStackView class not found");
                return;
            }
            let stack_cls = stack_cls.unwrap();
            let mut sv: *const NSView = msg_send![&*child, superview];
            let mut found_stack: *const NSView = std::ptr::null();
            let mut depth = 0;
            while !sv.is_null() && depth < 10 {
                let is_stack: bool = msg_send![sv, isKindOfClass: stack_cls];
                if is_stack {
                    found_stack = sv;
                    break;
                }
                sv = msg_send![sv, superview];
                depth += 1;
            }
            if found_stack.is_null() {
                eprintln!("match_parent_width: no NSStackView ancestor found");
                return;
            }
            let child_width: Retained<AnyObject> = msg_send![&*child, widthAnchor];
            let stack_width: Retained<AnyObject> = msg_send![found_stack, widthAnchor];
            let c: Retained<AnyObject> =
                msg_send![&*child_width, constraintEqualToAnchor: &*stack_width];
            let _: () = msg_send![&*c, setActive: true];
        }
    }
}

/// Pin a child view's top and bottom anchors to its superview, forcing it to
/// fill the parent's height.  Useful for HStack children that should stretch
/// vertically instead of being centered.
pub fn match_parent_height(child_handle: i64) {
    if let Some(child) = get_widget(child_handle) {
        unsafe {
            let superview_ptr: *const NSView = msg_send![&*child, superview];
            if superview_ptr.is_null() {
                eprintln!("match_parent_height: view has no superview");
                return;
            }
            let child_top: Retained<AnyObject> = msg_send![&*child, topAnchor];
            let child_bottom: Retained<AnyObject> = msg_send![&*child, bottomAnchor];
            let parent_top: Retained<AnyObject> = msg_send![superview_ptr, topAnchor];
            let parent_bottom: Retained<AnyObject> = msg_send![superview_ptr, bottomAnchor];
            let top_c: Retained<AnyObject> =
                msg_send![&*child_top, constraintEqualToAnchor: &*parent_top];
            let bot_c: Retained<AnyObject> =
                msg_send![&*child_bottom, constraintEqualToAnchor: &*parent_bottom];
            let _: () = msg_send![&*top_c, setActive: true];
            let _: () = msg_send![&*bot_c, setActive: true];
        }
    }
}

/// Pin a child view's leading and trailing anchors to its superview, forcing it
/// to fill the parent's width.
pub fn match_parent_width(child_handle: i64) {
    if let Some(child) = get_widget(child_handle) {
        unsafe {
            let superview_ptr: *const NSView = msg_send![&*child, superview];
            if superview_ptr.is_null() {
                eprintln!("match_parent_width: view has no superview");
                return;
            }
            let child_leading: Retained<AnyObject> = msg_send![&*child, leadingAnchor];
            let child_trailing: Retained<AnyObject> = msg_send![&*child, trailingAnchor];
            let parent_leading: Retained<AnyObject> = msg_send![superview_ptr, leadingAnchor];
            let parent_trailing: Retained<AnyObject> = msg_send![superview_ptr, trailingAnchor];
            let lead_c: Retained<AnyObject> =
                msg_send![&*child_leading, constraintEqualToAnchor: &*parent_leading];
            let trail_c: Retained<AnyObject> =
                msg_send![&*child_trailing, constraintEqualToAnchor: &*parent_trailing];
            let _: () = msg_send![&*lead_c, setActive: true];
            let _: () = msg_send![&*trail_c, setActive: true];
        }
    }
}

// =============================================================================
// Cross-cutting: Enabled, Hover, DoubleClick, Animations, Tooltip, ControlSize
// =============================================================================

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Set the enabled state of any NSControl-based widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setEnabled: enabled];
        }
    }
}

/// Set a tooltip on any widget.
pub fn set_tooltip(handle: i64, text: &str) {
    if let Some(view) = get_widget(handle) {
        let ns_text = objc2_foundation::NSString::from_str(text);
        unsafe {
            let _: () = objc2::msg_send![&*view, setToolTip: &*ns_text];
        }
    }
}

/// Set control size variant on NSControl-based widgets.
/// 0=regular, 1=small, 2=mini, 3=large
pub fn set_control_size(handle: i64, size: i64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setControlSize: size as u64];
        }
    }
}

/// Animate the opacity of a widget. `duration_secs` is in seconds.
pub fn animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            // Use NSAnimationContext
            let ctx_cls = AnyClass::get(c"NSAnimationContext").unwrap();
            let _: () = objc2::msg_send![ctx_cls, beginGrouping];
            let ctx: *mut AnyObject = objc2::msg_send![ctx_cls, currentContext];
            let _: () = objc2::msg_send![ctx, setDuration: duration_secs];
            let _: () = objc2::msg_send![ctx, setAllowsImplicitAnimation: true];
            let _: () = objc2::msg_send![&*view, setAlphaValue: target];
            let _: () = objc2::msg_send![ctx_cls, endGrouping];
        }
    }
}

/// Animate the position of a widget by delta. `duration_secs` is in seconds.
pub fn animate_position(handle: i64, dx: f64, dy: f64, duration_secs: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let frame: objc2_core_foundation::CGRect = objc2::msg_send![&*view, frame];
            let new_origin =
                objc2_core_foundation::CGPoint::new(frame.origin.x + dx, frame.origin.y + dy);
            let ctx_cls = AnyClass::get(c"NSAnimationContext").unwrap();
            let _: () = objc2::msg_send![ctx_cls, beginGrouping];
            let ctx: *mut AnyObject = objc2::msg_send![ctx_cls, currentContext];
            let _: () = objc2::msg_send![ctx, setDuration: duration_secs];
            let _: () = objc2::msg_send![ctx, setAllowsImplicitAnimation: true];
            let _: () = objc2::msg_send![&*view, setFrameOrigin: new_origin];
            let _: () = objc2::msg_send![ctx_cls, endGrouping];
        }
    }
}

use std::collections::HashMap;

thread_local! {
    static HOVER_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static DOUBLE_CLICK_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static CLICK_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

/// Set an on-hover callback for a widget (mouse enter/exit).
pub fn set_on_hover(handle: i64, callback: f64) {
    HOVER_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(handle, callback);
    });

    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
        }
        unsafe {
            perry_geisterhand_register(handle, 0, 3, callback, std::ptr::null());
        }
    }

    if let Some(view) = get_widget(handle) {
        unsafe {
            // Add tracking area for mouse enter/exit
            let ta_cls = AnyClass::get(c"NSTrackingArea").unwrap();
            let bounds: objc2_core_foundation::CGRect = objc2::msg_send![&*view, bounds];
            let options: u64 = 0x01 | 0x02 | 0x20; // MouseEnteredAndExited | MouseMoved | ActiveAlways
            let tracking_area: *mut AnyObject = objc2::msg_send![ta_cls, alloc];
            let tracking_area: *mut AnyObject = objc2::msg_send![
                tracking_area, initWithRect: bounds, options: options, owner: &*view, userInfo: std::ptr::null::<AnyObject>()
            ];
            let _: () = objc2::msg_send![&*view, addTrackingArea: tracking_area];
        }
    }
}

/// Set a double-click handler for a widget.
pub fn set_on_double_click(handle: i64, callback: f64) {
    DOUBLE_CLICK_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(handle, callback);
    });

    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
        }
        unsafe {
            perry_geisterhand_register(handle, 0, 4, callback, std::ptr::null());
        }
    }

    if let Some(view) = get_widget(handle) {
        unsafe {
            let gr_cls = AnyClass::get(c"NSClickGestureRecognizer").unwrap();
            let recognizer: *mut AnyObject = objc2::msg_send![gr_cls, alloc];
            let recognizer: *mut AnyObject = objc2::msg_send![recognizer, init];
            let _: () = objc2::msg_send![recognizer, setNumberOfClicksRequired: 2i64];
            let _: () = objc2::msg_send![&*view, addGestureRecognizer: recognizer];
        }
    }
}

// =============================================================================
// Single-click handler for any widget
// =============================================================================

/// Internal state for click gesture target
pub struct PerryClickTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

objc2::define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[name = "PerryClickTarget"]
    #[ivars = PerryClickTargetIvars]
    pub struct PerryClickTarget;

    impl PerryClickTarget {
        #[unsafe(method(handleClick:))]
        fn handle_click(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("click callback", std::panic::AssertUnwindSafe(|| {
                let key = self.ivars().callback_key.get();
                let closure_f64 = CLICK_CALLBACKS.with(|cbs| {
                    cbs.borrow().get(&key).copied()
                });
                if let Some(closure_f64) = closure_f64 {
                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call0(closure_ptr as *const u8);
                    }
                }
            }));
        }
    }
);

impl PerryClickTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryClickTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

/// Set a single-click handler for any widget.
pub fn set_on_click(handle: i64, callback: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            // Create target object for the gesture recognizer
            let target = PerryClickTarget::new();
            let target_addr = Retained::as_ptr(&target) as usize;
            target.ivars().callback_key.set(target_addr);

            // Store callback keyed by target address
            CLICK_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(target_addr, callback);
            });

            // Create NSClickGestureRecognizer with target-action
            let sel = objc2::runtime::Sel::register(c"handleClick:");
            let gr_cls = AnyClass::get(c"NSClickGestureRecognizer").unwrap();
            let recognizer: *mut AnyObject = objc2::msg_send![gr_cls, alloc];
            let recognizer: *mut AnyObject = objc2::msg_send![
                recognizer, initWithTarget: &*target, action: sel
            ];
            let _: () = objc2::msg_send![recognizer, setNumberOfClicksRequired: 1i64];
            let _: () = objc2::msg_send![&*view, addGestureRecognizer: recognizer];

            // Leak target to keep it alive
            std::mem::forget(target);

            #[cfg(feature = "geisterhand")]
            {
                extern "C" {
                    fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
                }
                perry_geisterhand_register(handle, 0, 0, callback, std::ptr::null());
            }
        }
    }
}
