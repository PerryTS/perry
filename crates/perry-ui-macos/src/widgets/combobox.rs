//! macOS Combobox widget (issue #475).
//!
//! Wraps `NSComboBox` (an `NSTextField` subclass with a built-in
//! dropdown). Supports as-you-type completion via `setCompletes:YES`,
//! emits the `on_change` callback both when an item is picked from the
//! dropdown (via `controlTextDidChange:` notification observation) and
//! when the user commits free text by pressing Return (target-action).
//!
//! See `picker.rs` for the sibling `NSPopUpButton` widget — combobox is
//! the editable + filterable variant.

use crate::ffi::js_string_from_bytes;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static COMBOBOX_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

pub struct PerryComboboxTargetIvars {
    pub handle: std::cell::Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryComboboxTarget"]
    #[ivars = PerryComboboxTargetIvars]
    pub struct PerryComboboxTarget;

    impl PerryComboboxTarget {
        #[unsafe(method(comboboxChanged:))]
        fn combobox_changed(&self, _sender: &AnyObject) {
            fire_callback(self as *const Self as usize, self.ivars().handle.get());
        }

        #[unsafe(method(comboboxSelectionChanged:))]
        fn combobox_selection_changed(&self, _note: &AnyObject) {
            fire_callback(self as *const Self as usize, self.ivars().handle.get());
        }
    }
);

impl PerryComboboxTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryComboboxTargetIvars {
            handle: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

fn fire_callback(target_addr: usize, handle: i64) {
    crate::catch_callback_panic(
        "combobox callback",
        std::panic::AssertUnwindSafe(|| {
            let cb = COMBOBOX_CALLBACKS.with(|cbs| cbs.borrow().get(&target_addr).copied());
            let Some(callback) = cb else { return };
            let Some(view) = super::get_widget(handle) else {
                return;
            };
            unsafe {
                let ns_str: Retained<NSString> = msg_send![&*view, stringValue];
                let bytes = ns_str.to_string();
                let header_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                let arg = js_nanbox_string(header_ptr as i64);
                let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
                js_closure_call1(closure_ptr, arg);
            }
        }),
    );
}

/// Create an `NSComboBox` with completion enabled. `initial_ptr` is a
/// StringHeader for the starting text (may be empty). `on_change` is a
/// NaN-boxed closure invoked with the current string value when the
/// user picks from the dropdown or commits free text via Return.
pub fn create(initial_ptr: *const u8, on_change: f64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let cls = AnyClass::get(c"NSComboBox").unwrap();
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(220.0, 25.0),
        );
        let raw: *mut AnyObject = msg_send![alloc, initWithFrame: frame];
        let combobox: Retained<AnyObject> = Retained::from_raw(raw).expect("NSComboBox init nil");

        let _: () = msg_send![&*combobox, setCompletes: true];
        let _: () = msg_send![&*combobox, setUsesDataSource: false];
        let _: () = msg_send![&*combobox, setEditable: true];
        let _: () = msg_send![&*combobox, setNumberOfVisibleItems: 8i64];

        let initial_str = str_from_header(initial_ptr);
        if !initial_str.is_empty() {
            let ns_initial = NSString::from_str(initial_str);
            let _: () = msg_send![&*combobox, setStringValue: &*ns_initial];
        }

        // Cast Retained<AnyObject> → Retained<NSView> for the registry.
        let view: Retained<NSView> = Retained::cast_unchecked(combobox);
        let handle = super::register_widget(view);

        let target = PerryComboboxTarget::new();
        target.ivars().handle.set(handle);
        let target_addr = Retained::as_ptr(&target) as usize;
        COMBOBOX_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_change);
        });

        let combobox_view = super::get_widget(handle).unwrap();
        let sel = Sel::register(c"comboboxChanged:");
        let _: () = msg_send![&*combobox_view, setTarget: &*target];
        let _: () = msg_send![&*combobox_view, setAction: sel];

        // Listen for dropdown selection notifications too — picking an
        // item without pressing Return wouldn't otherwise fire the
        // target-action.
        let nc_cls = AnyClass::get(c"NSNotificationCenter").unwrap();
        let nc: *mut AnyObject = msg_send![nc_cls, defaultCenter];
        let sel_sel = Sel::register(c"comboboxSelectionChanged:");
        let name = NSString::from_str("NSComboBoxSelectionDidChangeNotification");
        let _: () = msg_send![
            nc,
            addObserver: &*target,
            selector: sel_sel,
            name: &*name,
            object: &*combobox_view
        ];

        std::mem::forget(target);

        let _ = mtm;
        handle
    }
}

/// Append a suggestion item to the combobox dropdown.
pub fn add_item(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_val = NSString::from_str(value);
        unsafe {
            let _: () = msg_send![&*view, addItemWithObjectValue: &*ns_val];
        }
    }
}

/// Replace the editable text content of the combobox.
pub fn set_value(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_val = NSString::from_str(value);
        unsafe {
            let _: () = msg_send![&*view, setStringValue: &*ns_val];
        }
    }
}

/// Get the current editable text content as a NaN-boxed string handle
/// (`STRING_TAG`-tagged f64).
pub fn get_value(handle: i64) -> f64 {
    let Some(view) = super::get_widget(handle) else {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    };
    unsafe {
        let ns_str: Retained<NSString> = msg_send![&*view, stringValue];
        let bytes = ns_str.to_string();
        let header_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        js_nanbox_string(header_ptr as i64)
    }
}

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
