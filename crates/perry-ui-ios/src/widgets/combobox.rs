//! iOS Combobox widget (issue #475).
//!
//! UIKit has no `NSComboBox` analogue, so we build one from primitives:
//! a `UITextField` whose `inputView` is a `UIPickerView` driven by a
//! per-handle suggestion list. Free-text typing stays in the field;
//! tapping the field shows the picker (the iOS keyboard slot displays
//! the wheel); selecting an item populates the field. `on_change` fires
//! on every text edit and on picker selection — same semantics as the
//! macOS `NSComboBox` `controlTextDidChange:` notification.
//!
//! FFI surface mirrors the macOS impl in `perry-ui-macos`:
//!   - `create(initial, on_change)`
//!   - `add_item(handle, value)`
//!   - `set_value(handle, value)`
//!   - `get_value(handle) -> f64` (NaN-boxed string)

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::{UITextField, UIView};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    /// Suggestion list per widget handle (the picker rows).
    static COMBOBOX_ITEMS: RefCell<HashMap<i64, Vec<String>>> = RefCell::new(HashMap::new());
    /// on_change callback (NaN-boxed closure) per widget handle.
    static COMBOBOX_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    /// Cached delegate per widget handle — has to outlive the field;
    /// owned here, the UIPickerView holds it as a weak reference.
    static COMBOBOX_DELEGATES: RefCell<HashMap<i64, Retained<PerryComboboxDelegate>>> = RefCell::new(HashMap::new());
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

fn fire_callback(handle: i64, value: &str) {
    let cb = COMBOBOX_CALLBACKS.with(|m| m.borrow().get(&handle).copied());
    let Some(callback) = cb else { return };
    crate::catch_callback_panic(
        "combobox callback",
        std::panic::AssertUnwindSafe(|| unsafe {
            let bytes = value.as_bytes();
            let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            let arg = js_nanbox_string(header as i64);
            let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
            js_closure_call1(closure_ptr, arg);
        }),
    );
}

// ===========================================================================
// PerryComboboxDelegate — one object plays both UIPickerViewDataSource and
// UIPickerViewDelegate, plus the UIControlEventEditingChanged target action
// for the underlying UITextField. Mirrors the dual-role
// `PerryComboboxTarget` from the macOS impl.
// ===========================================================================

pub struct PerryComboboxDelegateIvars {
    handle: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryComboboxDelegateIOS"]
    #[ivars = PerryComboboxDelegateIvars]
    pub struct PerryComboboxDelegate;

    impl PerryComboboxDelegate {
        // UIPickerViewDataSource: number of wheel columns (always 1).
        #[unsafe(method(numberOfComponentsInPickerView:))]
        fn number_of_components(&self, _picker: &AnyObject) -> i64 {
            1
        }

        // UIPickerViewDataSource: row count for the single column.
        #[unsafe(method(pickerView:numberOfRowsInComponent:))]
        fn number_of_rows(&self, _picker: &AnyObject, _component: i64) -> i64 {
            let h = self.ivars().handle.get();
            COMBOBOX_ITEMS.with(|m| m.borrow().get(&h).map(|v| v.len() as i64).unwrap_or(0))
        }

        // UIPickerViewDelegate: title text for a given row.
        #[unsafe(method(pickerView:titleForRow:forComponent:))]
        fn title_for_row(
            &self,
            _picker: &AnyObject,
            row: i64,
            _component: i64,
        ) -> *mut AnyObject {
            let h = self.ivars().handle.get();
            let title = COMBOBOX_ITEMS.with(|m| {
                m.borrow()
                    .get(&h)
                    .and_then(|v| v.get(row as usize).cloned())
            });
            let Some(title) = title else {
                return std::ptr::null_mut();
            };
            let ns: Retained<NSString> = NSString::from_str(&title);
            // Hand back as autorelease; UIKit will retain.
            Retained::into_raw(ns) as *mut AnyObject
        }

        // UIPickerViewDelegate: user picked a row — copy into the field and fire.
        #[unsafe(method(pickerView:didSelectRow:inComponent:))]
        fn did_select_row(&self, _picker: &AnyObject, row: i64, _component: i64) {
            let h = self.ivars().handle.get();
            let value = COMBOBOX_ITEMS.with(|m| {
                m.borrow()
                    .get(&h)
                    .and_then(|v| v.get(row as usize).cloned())
            });
            let Some(value) = value else { return };
            if let Some(view) = super::get_widget(h) {
                let ns = NSString::from_str(&value);
                unsafe {
                    let _: () = msg_send![&*view, setText: &*ns];
                }
            }
            fire_callback(h, &value);
        }

        // UIControlEventEditingChanged target action — fires while user types.
        #[unsafe(method(comboboxTextChanged:))]
        fn text_changed(&self, sender: &AnyObject) {
            let h = self.ivars().handle.get();
            unsafe {
                let ns: Retained<NSString> = msg_send![sender, text];
                let s = ns.to_string();
                fire_callback(h, &s);
            }
        }
    }
);

impl PerryComboboxDelegate {
    fn new(handle: i64) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryComboboxDelegateIvars {
            handle: Cell::new(handle),
        });
        unsafe { msg_send![super(this), init] }
    }
}

// ===========================================================================
// Public API
// ===========================================================================

/// Create a combobox: a `UITextField` with a `UIPickerView` as its
/// `inputView`. Returns a 1-based widget handle.
pub fn create(initial_ptr: *const u8, on_change: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let text_field: Retained<UITextField> =
            msg_send![AnyClass::get(c"UITextField").unwrap(), new];
        // Match macOS combobox default chrome: rounded rect, 220×25 hint.
        let _: () = msg_send![&*text_field, setBorderStyle: 3i64]; // RoundedRect
        let _: () = msg_send![&*text_field, setTranslatesAutoresizingMaskIntoConstraints: false];

        let initial = str_from_header(initial_ptr);
        if !initial.is_empty() {
            let ns = NSString::from_str(initial);
            let _: () = msg_send![&*text_field, setText: &*ns];
        }

        let view: Retained<UIView> = Retained::cast_unchecked(text_field);
        let handle = super::register_widget(view);

        COMBOBOX_ITEMS.with(|m| {
            m.borrow_mut().insert(handle, Vec::new());
        });
        COMBOBOX_CALLBACKS.with(|m| {
            m.borrow_mut().insert(handle, on_change);
        });

        // UIPickerView as inputView — when the field becomes first
        // responder, iOS shows the wheel in place of the keyboard.
        let picker_cls = AnyClass::get(c"UIPickerView").unwrap();
        let picker_alloc: *mut AnyObject = msg_send![picker_cls, alloc];
        let picker_raw: *mut AnyObject = msg_send![picker_alloc, init];
        let picker: Retained<AnyObject> =
            Retained::from_raw(picker_raw).expect("UIPickerView init nil");

        let delegate = PerryComboboxDelegate::new(handle);
        let _: () = msg_send![&*picker, setDataSource: &*delegate];
        let _: () = msg_send![&*picker, setDelegate: &*delegate];

        // Attach the picker as the keyboard replacement.
        let field_view = super::get_widget(handle).unwrap();
        let _: () = msg_send![&*field_view, setInputView: &*picker];

        // UIControlEventEditingChanged = 1 << 16 = 65536 — fires on every
        // character. Matches `controlTextDidChange:` on AppKit.
        let sel = Sel::register(c"comboboxTextChanged:");
        let _: () =
            msg_send![&*field_view, addTarget: &*delegate, action: sel, forControlEvents: 65536u64];

        // The picker holds the delegate as a weak pointer; the
        // UITextField target-action ref is also weak. Park a strong
        // reference in the per-handle map so neither callback site
        // dangles.
        COMBOBOX_DELEGATES.with(|m| {
            m.borrow_mut().insert(handle, delegate);
        });

        // Forget the picker — it's now retained by the text field's
        // inputView. Without this, the Retained drop would free it.
        std::mem::forget(picker);

        handle
    }
}

/// Append a suggestion to the dropdown. Triggers a picker reload so the
/// new row appears immediately if the wheel is currently visible.
pub fn add_item(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr).to_string();
    COMBOBOX_ITEMS.with(|m| {
        if let Some(list) = m.borrow_mut().get_mut(&handle) {
            list.push(value);
        }
    });
    // Refresh the wheel data — `reloadAllComponents` is safe to call
    // even when the picker is offscreen.
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let picker: *mut AnyObject = msg_send![&*view, inputView];
            if !picker.is_null() {
                let _: () = msg_send![picker, reloadAllComponents];
            }
        }
    }
}

/// Replace the editable text content of the combobox. Does not fire
/// `on_change` — matches macOS `setStringValue:` semantics.
pub fn set_value(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns = NSString::from_str(value);
        unsafe {
            let _: () = msg_send![&*view, setText: &*ns];
        }
    }
}

/// Get the current editable text content as a NaN-boxed string.
pub fn get_value(handle: i64) -> f64 {
    let Some(view) = super::get_widget(handle) else {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    };
    unsafe {
        let ns: Retained<NSString> = msg_send![&*view, text];
        let s = ns.to_string();
        let bytes = s.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}
