//! Issue #553 — `BottomNavigation` on iOS using UITabBar.
//!
//! Distinct from the existing `tabbar.rs` (legacy, simpler API) — this
//! widget exposes the BottomNavigation surface from the issue: items
//! described as `{icon, label, badge?}` with a reactive selectedIndex
//! and an onSelect callback. Both can coexist.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::UIView;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

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

struct BottomNavState {
    items: Vec<*mut AnyObject>,
    on_select: f64,
}

thread_local! {
    static STATES: RefCell<HashMap<i64, BottomNavState>> = RefCell::new(HashMap::new());
    static DELEGATE_TO_HANDLE: RefCell<HashMap<usize, i64>> = RefCell::new(HashMap::new());
}

pub struct PerryBottomNavDelegateIvars {
    key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryBottomNavDelegate"]
    #[ivars = PerryBottomNavDelegateIvars]
    pub struct PerryBottomNavDelegate;

    impl PerryBottomNavDelegate {
        #[unsafe(method(tabBar:didSelectItem:))]
        fn tab_bar_did_select_item(&self, _tab_bar: &AnyObject, item: &AnyObject) {
            let tag: i64 = unsafe { msg_send![item, tag] };
            let key = self.ivars().key.get();
            let handle = DELEGATE_TO_HANDLE.with(|m| m.borrow().get(&key).copied().unwrap_or(0));
            if handle == 0 { return; }
            let on_select = STATES.with(|s| {
                s.borrow().get(&handle).map(|st| st.on_select).unwrap_or(0.0)
            });
            if on_select != 0.0 {
                let pkg = Box::new((on_select, tag));
                unsafe {
                    dispatch_async_f(
                        &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                        Box::into_raw(pkg) as *mut std::ffi::c_void,
                        trampoline,
                    );
                }
            }
        }
    }
);

impl PerryBottomNavDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryBottomNavDelegateIvars {
            key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

unsafe extern "C" fn trampoline(ctx: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let pkg = Box::from_raw(ctx as *mut (f64, i64));
        let (closure, idx) = *pkg;
        let ptr = js_nanbox_get_pointer(closure) as *const u8;
        js_closure_call1(ptr, idx as f64);
    });
}

/// Create a BottomNavigation bar (UITabBar). Items added via `add_item`.
pub fn create(on_select: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let cls = objc2::runtime::AnyClass::get(c"UITabBar").unwrap();
        let bar: *mut AnyObject = msg_send![cls, alloc];
        let bar: *mut AnyObject = msg_send![bar, init];

        let delegate = PerryBottomNavDelegate::new();
        let key = Retained::as_ptr(&delegate) as usize;
        delegate.ivars().key.set(key);
        let _: () = msg_send![bar, setDelegate: &*delegate];
        std::mem::forget(delegate);

        let view: Retained<UIView> = Retained::retain(bar as *mut UIView).unwrap();
        let handle = super::register_widget(view);

        DELEGATE_TO_HANDLE.with(|m| {
            m.borrow_mut().insert(key, handle);
        });
        STATES.with(|s| {
            s.borrow_mut().insert(
                handle,
                BottomNavState {
                    items: Vec::new(),
                    on_select,
                },
            );
        });
        handle
    }
}

/// Add an item with an SF Symbol icon and a label.
pub fn add_item(handle: i64, icon_ptr: *const u8, label_ptr: *const u8) {
    let icon = str_from_header(icon_ptr);
    let label = str_from_header(label_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    let tab_index = STATES.with(|s| {
        s.borrow()
            .get(&handle)
            .map(|st| st.items.len() as i64)
            .unwrap_or(0)
    });

    unsafe {
        let img_cls = objc2::runtime::AnyClass::get(c"UIImage").unwrap();
        let ns_icon = NSString::from_str(icon);
        let image: *mut AnyObject = msg_send![img_cls, systemImageNamed: &*ns_icon];

        let item_cls = objc2::runtime::AnyClass::get(c"UITabBarItem").unwrap();
        let item: *mut AnyObject = msg_send![item_cls, alloc];
        let ns_title = NSString::from_str(label);
        let item: *mut AnyObject = msg_send![
            item,
            initWithTitle: &*ns_title,
            image: image,
            tag: tab_index
        ];

        STATES.with(|s| {
            if let Some(state) = s.borrow_mut().get_mut(&handle) {
                state.items.push(item);
                if let Some(bar) = super::get_widget(handle) {
                    let arr_cls = objc2::runtime::AnyClass::get(c"NSMutableArray").unwrap();
                    let arr: *mut AnyObject =
                        msg_send![arr_cls, arrayWithCapacity: state.items.len()];
                    for &it in &state.items {
                        let _: () = msg_send![arr, addObject: it];
                    }
                    let _: () = msg_send![&*bar, setItems: arr, animated: false];
                }
            }
        });
    }
}

/// Set or clear the badge string on an item. Empty string clears.
pub fn set_badge(handle: i64, index: i64, badge_ptr: *const u8) {
    let badge = str_from_header(badge_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    STATES.with(|s| {
        let state_ref = s.borrow();
        let Some(state) = state_ref.get(&handle) else {
            return;
        };
        let Some(&item) = state.items.get(index as usize) else {
            return;
        };
        unsafe {
            let badge_value: *const AnyObject = if badge.is_empty() {
                std::ptr::null()
            } else {
                let ns_badge = NSString::from_str(badge);
                Retained::into_raw(ns_badge) as *const AnyObject
            };
            let _: () = msg_send![item, setBadgeValue: badge_value];
        }
    });
}

/// Programmatically select a tab. Does NOT fire the on-select callback.
pub fn set_selected(handle: i64, index: i64) {
    STATES.with(|s| {
        let state_ref = s.borrow();
        let Some(state) = state_ref.get(&handle) else {
            return;
        };
        let Some(&item) = state.items.get(index as usize) else {
            return;
        };
        if let Some(bar) = super::get_widget(handle) {
            unsafe {
                let _: () = msg_send![&*bar, setSelectedItem: item];
            }
        }
    });
}
