//! System tray icon (issue #490).
//!
//! Wraps `NSStatusItem` from `NSStatusBar.system`. The visible icon is the
//! `.button` on the status item; clicking it either opens the attached
//! `NSMenu` (set via `trayAttachMenu`) or fires the JS click callback
//! (registered via `trayOnClick`).
//!
//! Handle-based dispatch matches `menu.rs` — 1-based indices into a
//! thread-local `Vec<Retained<NSStatusItem>>`.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{NSImage, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static TRAYS: RefCell<Vec<Retained<NSStatusItem>>> = const { RefCell::new(Vec::new()) };
    static TRAY_CLICK_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Extract a &str from a *const StringHeader pointer. Mirrors menu.rs.
fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

pub struct PerryTrayClickTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTrayClickTarget"]
    #[ivars = PerryTrayClickTargetIvars]
    pub struct PerryTrayClickTarget;

    impl PerryTrayClickTarget {
        #[unsafe(method(trayClicked:))]
        fn tray_clicked(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("tray click callback", std::panic::AssertUnwindSafe(|| {
                let key = self.ivars().callback_key.get();
                let closure_f64 = TRAY_CLICK_CALLBACKS.with(|cbs| {
                    cbs.borrow().get(&key).copied()
                });
                if let Some(cf) = closure_f64 {
                    let closure_ptr = unsafe { js_nanbox_get_pointer(cf) };
                    unsafe {
                        js_closure_call0(closure_ptr as *const u8);
                    }
                }
            }));
        }
    }
);

impl PerryTrayClickTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTrayClickTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

fn get_tray(handle: i64) -> Option<Retained<NSStatusItem>> {
    TRAYS.with(|t| {
        let trays = t.borrow();
        let idx = (handle - 1) as usize;
        trays.get(idx).cloned()
    })
}

/// Create a tray icon. `icon_path_ptr` is a UTF-8 path to a PNG / icns;
/// pass empty string to leave the button label-only (a "●" placeholder).
pub fn create(icon_path_ptr: *const u8) -> i64 {
    let path = str_from_header(icon_path_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let bar = NSStatusBar::systemStatusBar();
        let item = bar.statusItemWithLength(NSVariableStatusItemLength);

        if let Some(button) = item.button(mtm) {
            if !path.is_empty() {
                let ns_path = NSString::from_str(path);
                let image: Option<Retained<NSImage>> = msg_send![
                    NSImage::alloc(), initWithContentsOfFile: &*ns_path
                ];
                if let Some(image) = image {
                    // Status-bar icons should respect light/dark mode unless the
                    // user supplies a multi-color asset — set template by default.
                    image.setTemplate(true);
                    button.setImage(Some(&image));
                } else {
                    let title = NSString::from_str("●");
                    button.setTitle(&title);
                }
            } else {
                let title = NSString::from_str("●");
                button.setTitle(&title);
            }
        }

        TRAYS.with(|t| {
            let mut trays = t.borrow_mut();
            trays.push(item);
            trays.len() as i64
        })
    }
}

pub fn set_icon(handle: i64, icon_path_ptr: *const u8) {
    let path = str_from_header(icon_path_ptr);
    if path.is_empty() {
        return;
    }
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    if let Some(item) = get_tray(handle) {
        unsafe {
            if let Some(button) = item.button(mtm) {
                let ns_path = NSString::from_str(path);
                let image: Option<Retained<NSImage>> = msg_send![
                    NSImage::alloc(), initWithContentsOfFile: &*ns_path
                ];
                if let Some(image) = image {
                    image.setTemplate(true);
                    button.setImage(Some(&image));
                }
            }
        }
    }
}

pub fn set_tooltip(handle: i64, tooltip_ptr: *const u8) {
    let tooltip = str_from_header(tooltip_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    if let Some(item) = get_tray(handle) {
        let ns_tooltip = NSString::from_str(tooltip);
        // NSStatusItem.setToolTip is deprecated since 10.10; the modern
        // surface routes through the button's toolTip property.
        if let Some(button) = item.button(mtm) {
            button.setToolTip(Some(&ns_tooltip));
        }
    }
}

pub fn attach_menu(tray_handle: i64, menu_handle: i64) {
    if let Some(item) = get_tray(tray_handle) {
        // Reuse the menu module's storage. NSStatusItem has a `menu` property
        // that auto-pops on click — no manual click handler needed.
        if let Some(menu) = crate::menu::get_menu_for_tray(menu_handle) {
            item.setMenu(Some(&menu));
        }
    }
}

pub fn on_click(tray_handle: i64, callback: f64) {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    if let Some(item) = get_tray(tray_handle) {
        unsafe {
            if let Some(button) = item.button(mtm) {
                let target = PerryTrayClickTarget::new();
                let target_addr = Retained::as_ptr(&target) as usize;
                target.ivars().callback_key.set(target_addr);

                TRAY_CLICK_CALLBACKS.with(|cbs| {
                    cbs.borrow_mut().insert(target_addr, callback);
                });

                // NSStatusBarButton inherits from NSControl → setTarget/setAction.
                let _: () = msg_send![&*button, setTarget: &*target];
                let _: () = msg_send![&*button, setAction: Sel::register(c"trayClicked:")];
                std::mem::forget(target);
            }
        }
    }
}

pub fn destroy(handle: i64) {
    if let Some(item) = get_tray(handle) {
        let bar = NSStatusBar::systemStatusBar();
        bar.removeStatusItem(&item);
        // We deliberately keep the slot occupied so handle indices remain
        // stable (matches the Vec-based menu/widget pattern). A second call
        // to `setMenu`/`setIcon` on a destroyed handle is a no-op because
        // the item is no longer in the bar.
    }
}
