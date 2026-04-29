use gtk4::prelude::*;
use gtk4::{CheckButton, Label, Orientation};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from toggle ID to closure pointer (f64 NaN-boxed)
    static TOGGLE_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    /// Map from toggle widget handle -> CheckButton for two-way binding
    static TOGGLE_CHECKS: RefCell<HashMap<i64, CheckButton>> = RefCell::new(HashMap::new());
    static NEXT_TOGGLE_ID: RefCell<usize> = RefCell::new(1);
}

// TAG_TRUE and TAG_FALSE from perry-runtime NaN-boxing
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Extract a &str from a *const StringHeader pointer.
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

/// Set the on/off state of an existing toggle widget.
pub fn set_state(handle: i64, on: i64) {
    TOGGLE_CHECKS.with(|checks| {
        if let Some(check) = checks.borrow().get(&handle) {
            check.set_active(on != 0);
        }
    });
}

/// Create a CheckButton with a label and onChange callback.
/// Returns a widget handle for an HStack containing the label and checkbox.
pub fn create(label_ptr: *const u8, on_change: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let label_text = str_from_header(label_ptr);

    let check = CheckButton::new();
    let label = Label::new(Some(label_text));

    let callback_id = NEXT_TOGGLE_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    TOGGLE_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(callback_id, on_change);
    });

    check.connect_toggled(move |check| {
        let closure_f64 = TOGGLE_CALLBACKS.with(|cbs| cbs.borrow().get(&callback_id).copied());
        if let Some(closure_f64) = closure_f64 {
            let value = if check.is_active() {
                f64::from_bits(TAG_TRUE)
            } else {
                f64::from_bits(TAG_FALSE)
            };

            let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
            unsafe {
                js_closure_call1(closure_ptr as *const u8, value);
            }
        }
    });

    // Create HStack containing label + checkbox
    let hbox = gtk4::Box::new(Orientation::Horizontal, 8);
    hbox.append(&label);
    hbox.append(&check);

    let handle = super::register_widget(hbox.upcast());

    // Store the CheckButton reference for two-way binding (set_state)
    TOGGLE_CHECKS.with(|checks| {
        checks.borrow_mut().insert(handle, check);
    });

    handle
}
