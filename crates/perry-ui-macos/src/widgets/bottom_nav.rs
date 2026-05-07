//! Issue #553 — `BottomNavigation` (5-tab bottom bar with icon + label + badge).
//!
//! macOS has no native bottom-nav primitive (NSTabView is top-aligned and
//! styled differently), so this builds the bar manually as an NSStackView
//! of NSButton tab items. Each item shows an SF Symbol on top of a small
//! label; the currently selected item gets a tinted color and the others
//! show in a muted gray. Optional badge is drawn as a red NSTextField
//! pinned to the top-right of the icon.
//!
//! On iOS the equivalent widget uses UITabBar / UITabBarItem natively
//! (see crates/perry-ui-ios/src/widgets/bottom_nav.rs).

use crate::string_header::StringHeader;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

struct ItemViews {
    container: Retained<NSView>,
    button: Retained<AnyObject>,
    icon_view: Retained<AnyObject>,
    label_view: Retained<AnyObject>,
    badge_view: Option<Retained<AnyObject>>,
}

struct BottomNavState {
    bar_view: Retained<NSView>,
    items: Vec<ItemViews>,
    on_select: f64,
    selected_index: i64,
}

thread_local! {
    static BOTTOM_NAVS: RefCell<HashMap<i64, BottomNavState>> = RefCell::new(HashMap::new());
    static TARGET_TO_HANDLE: RefCell<HashMap<usize, (i64, i64)>> = RefCell::new(HashMap::new());
}

pub struct PerryBottomNavTargetIvars {
    key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryBottomNavTarget"]
    #[ivars = PerryBottomNavTargetIvars]
    pub struct PerryBottomNavTarget;

    impl PerryBottomNavTarget {
        #[unsafe(method(handleTap:))]
        fn handle_tap(&self, _sender: &AnyObject) {
            let key = self.ivars().key.get();
            let (bar_handle, item_index) = TARGET_TO_HANDLE.with(|m| {
                m.borrow().get(&key).copied().unwrap_or((0, -1))
            });
            if bar_handle == 0 || item_index < 0 {
                return;
            }
            let on_select = BOTTOM_NAVS.with(|s| {
                s.borrow().get(&bar_handle).map(|st| st.on_select).unwrap_or(0.0)
            });
            select_index(bar_handle, item_index);
            if on_select != 0.0 {
                unsafe {
                    let closure_ptr = js_nanbox_get_pointer(on_select) as *const u8;
                    js_closure_call1(closure_ptr, item_index as f64);
                }
            }
        }
    }
);

impl PerryBottomNavTarget {
    fn new(key: usize) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryBottomNavTargetIvars {
            key: std::cell::Cell::new(key),
        });
        unsafe { msg_send![super(this), init] }
    }
}

const ITEM_WIDTH: f64 = 72.0;
const BAR_HEIGHT: f64 = 56.0;
const ICON_SIZE: f64 = 22.0;

/// Create a BottomNavigation bar. Items are added with `add_item`.
pub fn create(on_select: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let stack_cls = AnyClass::get(c"NSStackView").unwrap();
        let stack: Retained<AnyObject> = msg_send![stack_cls, new];
        // NSUserInterfaceLayoutOrientationHorizontal = 0
        let _: () = msg_send![&*stack, setOrientation: 0i64];
        // NSStackViewDistributionFillEqually = 1
        let _: () = msg_send![&*stack, setDistribution: 1i64];
        let _: () = msg_send![&*stack, setSpacing: 0.0f64];
        let _: () = msg_send![&*stack, setEdgeInsets: objc2_foundation::NSEdgeInsets {
            top: 4.0,
            left: 0.0,
            bottom: 4.0,
            right: 0.0,
        }];
        let _: () = msg_send![&*stack, setTranslatesAutoresizingMaskIntoConstraints: false];

        // Force a fixed bar height so the bottom-nav looks like a bar
        // rather than something that grows with the available space.
        let height_anchor: Retained<AnyObject> = msg_send![&*stack, heightAnchor];
        let constraint: Retained<AnyObject> =
            msg_send![&*height_anchor, constraintEqualToConstant: BAR_HEIGHT];
        let _: () = msg_send![&*constraint, setActive: true];

        let view: Retained<NSView> = Retained::cast_unchecked(stack.clone());
        let handle = super::register_widget(view.clone());

        BOTTOM_NAVS.with(|s| {
            s.borrow_mut().insert(
                handle,
                BottomNavState {
                    bar_view: view,
                    items: Vec::new(),
                    on_select,
                    selected_index: 0,
                },
            );
        });
        handle
    }
}

/// Add a tab item (icon + label) to a BottomNavigation bar.
pub fn add_item(bar_handle: i64, icon_ptr: *const u8, label_ptr: *const u8) {
    let icon = str_from_header(icon_ptr);
    let label = str_from_header(label_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        // Build the item container as a transparent NSButton with custom
        // subviews — gives us click handling for free + lets us draw the
        // icon/label/badge ourselves.
        let btn_cls = AnyClass::get(c"NSButton").unwrap();
        let button: Retained<AnyObject> = msg_send![btn_cls, new];
        let _: () = msg_send![&*button, setBordered: false];
        let _: () = msg_send![&*button, setTitle: &*NSString::from_str("")];
        // NSBezelStyleTexturedSquare == 11; we don't want any bezel so
        // setBordered:false is enough, but also set imagePosition to none
        // (the icon is a separate subview, not the button's image).
        let _: () = msg_send![&*button, setImagePosition: 0i64];
        let _: () = msg_send![&*button, setTranslatesAutoresizingMaskIntoConstraints: false];

        let item_index = BOTTOM_NAVS.with(|s| {
            s.borrow()
                .get(&bar_handle)
                .map(|st| st.items.len() as i64)
                .unwrap_or(0)
        });

        let target = PerryBottomNavTarget::new(0);
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().key.set(target_addr);
        TARGET_TO_HANDLE.with(|m| {
            m.borrow_mut().insert(target_addr, (bar_handle, item_index));
        });
        let action = Sel::register(c"handleTap:");
        let _: () = msg_send![&*button, setTarget: &*target];
        let _: () = msg_send![&*button, setAction: action];
        std::mem::forget(target);

        // Icon: NSImageView with SF Symbol if available.
        let iv_cls = AnyClass::get(c"NSImageView").unwrap();
        let icon_view: Retained<AnyObject> = msg_send![iv_cls, new];
        let _: () = msg_send![&*icon_view, setTranslatesAutoresizingMaskIntoConstraints: false];
        if !icon.is_empty() {
            let img_cls = AnyClass::get(c"NSImage").unwrap();
            let ns_icon = NSString::from_str(icon);
            let nil: *const AnyObject = std::ptr::null();
            let image: *mut AnyObject = msg_send![
                img_cls,
                imageWithSystemSymbolName: &*ns_icon,
                accessibilityDescription: nil
            ];
            if !image.is_null() {
                let _: () = msg_send![&*icon_view, setImage: image];
            }
        }

        // Label: small NSTextField with .controlTextColor.
        let tf_cls = AnyClass::get(c"NSTextField").unwrap();
        let ns_label = NSString::from_str(label);
        let label_view: Retained<AnyObject> = msg_send![tf_cls, labelWithString: &*ns_label];
        let _: () = msg_send![&*label_view, setTranslatesAutoresizingMaskIntoConstraints: false];
        let _: () = msg_send![&*label_view, setAlignment: 2i64]; // NSTextAlignmentCenter
        let font_cls = AnyClass::get(c"NSFont").unwrap();
        let font: Retained<AnyObject> = msg_send![font_cls, systemFontOfSize: 10.0f64];
        let _: () = msg_send![&*label_view, setFont: &*font];

        let _: () = msg_send![&*button, addSubview: &*icon_view];
        let _: () = msg_send![&*button, addSubview: &*label_view];

        // Constrain icon/label inside button.
        let btn_top: Retained<AnyObject> = msg_send![&*button, topAnchor];
        let btn_bot: Retained<AnyObject> = msg_send![&*button, bottomAnchor];
        let btn_cx: Retained<AnyObject> = msg_send![&*button, centerXAnchor];

        let icon_top: Retained<AnyObject> = msg_send![&*icon_view, topAnchor];
        let icon_w: Retained<AnyObject> = msg_send![&*icon_view, widthAnchor];
        let icon_h: Retained<AnyObject> = msg_send![&*icon_view, heightAnchor];
        let icon_cx: Retained<AnyObject> = msg_send![&*icon_view, centerXAnchor];

        let label_bot: Retained<AnyObject> = msg_send![&*label_view, bottomAnchor];
        let label_cx: Retained<AnyObject> = msg_send![&*label_view, centerXAnchor];
        let label_top: Retained<AnyObject> = msg_send![&*label_view, topAnchor];
        let icon_bot: Retained<AnyObject> = msg_send![&*icon_view, bottomAnchor];

        let constraints: [Retained<AnyObject>; 8] = [
            msg_send![&*icon_top, constraintEqualToAnchor: &*btn_top, constant: 4.0f64],
            msg_send![&*icon_w, constraintEqualToConstant: ICON_SIZE],
            msg_send![&*icon_h, constraintEqualToConstant: ICON_SIZE],
            msg_send![&*icon_cx, constraintEqualToAnchor: &*btn_cx],
            msg_send![&*label_bot, constraintEqualToAnchor: &*btn_bot, constant: -4.0f64],
            msg_send![&*label_cx, constraintEqualToAnchor: &*btn_cx],
            msg_send![&*label_top, constraintEqualToAnchor: &*icon_bot, constant: 2.0f64],
            msg_send![&*button, widthAnchor],
        ];
        // Activate the first 7; the 8th is the widthAnchor we re-use to set ITEM_WIDTH below.
        for c in &constraints[..7] {
            let _: () = msg_send![&**c, setActive: true];
        }
        let width_constraint: Retained<AnyObject> =
            msg_send![&*constraints[7], constraintGreaterThanOrEqualToConstant: ITEM_WIDTH];
        let _: () = msg_send![&*width_constraint, setActive: true];

        // Add to the stack.
        if let Some(bar_view) = super::get_widget(bar_handle) {
            let _: () = msg_send![&*bar_view, addArrangedSubview: &*button];
        }

        BOTTOM_NAVS.with(|s| {
            if let Some(state) = s.borrow_mut().get_mut(&bar_handle) {
                state.items.push(ItemViews {
                    container: Retained::cast_unchecked(button.clone()),
                    button,
                    icon_view,
                    label_view,
                    badge_view: None,
                });
            }
        });

        // Apply selected styling — first item is selected by default.
        apply_styling(bar_handle);
    }
}

/// Set or clear the badge text on a tab item. Empty string clears the badge.
pub fn set_badge(bar_handle: i64, index: i64, badge_ptr: *const u8) {
    let badge_str = str_from_header(badge_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    BOTTOM_NAVS.with(|s| {
        let mut nav = s.borrow_mut();
        let Some(state) = nav.get_mut(&bar_handle) else {
            return;
        };
        let Some(item) = state.items.get_mut(index as usize) else {
            return;
        };
        unsafe {
            // Remove old badge, if any.
            if let Some(old) = item.badge_view.take() {
                let _: () = msg_send![&*old, removeFromSuperview];
            }
            if badge_str.is_empty() {
                return;
            }

            let tf_cls = AnyClass::get(c"NSTextField").unwrap();
            let ns_badge = NSString::from_str(badge_str);
            let badge: Retained<AnyObject> = msg_send![tf_cls, labelWithString: &*ns_badge];
            let _: () = msg_send![&*badge, setTranslatesAutoresizingMaskIntoConstraints: false];
            let _: () = msg_send![&*badge, setAlignment: 2i64]; // center
            let _: () = msg_send![&*badge, setBordered: false];
            let _: () = msg_send![&*badge, setDrawsBackground: true];

            let color_cls = AnyClass::get(c"NSColor").unwrap();
            let red: Retained<AnyObject> = msg_send![
                color_cls,
                colorWithRed: 0.85f64,
                green: 0.20f64,
                blue: 0.20f64,
                alpha: 1.0f64
            ];
            let white: Retained<AnyObject> = msg_send![color_cls, whiteColor];
            let _: () = msg_send![&*badge, setBackgroundColor: &*red];
            let _: () = msg_send![&*badge, setTextColor: &*white];
            let font_cls = AnyClass::get(c"NSFont").unwrap();
            let font: Retained<AnyObject> = msg_send![font_cls, boldSystemFontOfSize: 9.0f64];
            let _: () = msg_send![&*badge, setFont: &*font];

            // NSTextField doesn't draw rounded corners by default; opt-in
            // via the wantsLayer / cornerRadius pair.
            let _: () = msg_send![&*badge, setWantsLayer: true];
            let layer: Retained<AnyObject> = msg_send![&*badge, layer];
            let _: () = msg_send![&*layer, setCornerRadius: 8.0f64];

            let _: () = msg_send![&*item.button, addSubview: &*badge];

            // Pin badge to top-right of the icon.
            let badge_top: Retained<AnyObject> = msg_send![&*badge, topAnchor];
            let badge_left: Retained<AnyObject> = msg_send![&*badge, leadingAnchor];
            let badge_h: Retained<AnyObject> = msg_send![&*badge, heightAnchor];
            let badge_w: Retained<AnyObject> = msg_send![&*badge, widthAnchor];
            let icon_top: Retained<AnyObject> = msg_send![&*item.icon_view, topAnchor];
            let icon_trail: Retained<AnyObject> = msg_send![&*item.icon_view, trailingAnchor];
            let cs: [Retained<AnyObject>; 4] = [
                msg_send![&*badge_top, constraintEqualToAnchor: &*icon_top, constant: -4.0f64],
                msg_send![&*badge_left, constraintEqualToAnchor: &*icon_trail, constant: -4.0f64],
                msg_send![&*badge_h, constraintEqualToConstant: 16.0f64],
                msg_send![&*badge_w, constraintGreaterThanOrEqualToConstant: 16.0f64],
            ];
            for c in &cs {
                let _: () = msg_send![&**c, setActive: true];
            }

            item.badge_view = Some(badge);
        }
    });
}

/// Programmatically select a tab. Does NOT fire the on-select callback.
pub fn set_selected(bar_handle: i64, index: i64) {
    select_index(bar_handle, index);
}

fn select_index(bar_handle: i64, index: i64) {
    BOTTOM_NAVS.with(|s| {
        if let Some(state) = s.borrow_mut().get_mut(&bar_handle) {
            if (index as usize) < state.items.len() {
                state.selected_index = index;
            }
        }
    });
    apply_styling(bar_handle);
}

fn apply_styling(bar_handle: i64) {
    BOTTOM_NAVS.with(|s| {
        let nav = s.borrow();
        let Some(state) = nav.get(&bar_handle) else {
            return;
        };
        unsafe {
            let color_cls = AnyClass::get(c"NSColor").unwrap();
            let selected: Retained<AnyObject> = msg_send![
                color_cls,
                colorWithRed: 0.000f64,
                green: 0.478f64,
                blue: 1.000f64,
                alpha: 1.0f64
            ];
            let muted: Retained<AnyObject> = msg_send![color_cls, secondaryLabelColor];
            for (i, item) in state.items.iter().enumerate() {
                let is_selected = i as i64 == state.selected_index;
                let color: &AnyObject = if is_selected { &*selected } else { &*muted };
                // Tint the icon symbol.
                let _: () = msg_send![&*item.icon_view, setContentTintColor: color];
                // Tint the label.
                let _: () = msg_send![&*item.label_view, setTextColor: color];
            }
        }
    });
}

#[allow(dead_code)]
fn _touch(state: &BottomNavState) -> *const AnyObject {
    Retained::as_ptr(&state.bar_view) as *const AnyObject
}

#[allow(dead_code)]
fn _touch_item(item: &ItemViews) -> *const AnyObject {
    Retained::as_ptr(&item.container) as *const AnyObject
}
