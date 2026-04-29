use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{
    NSLayoutAttribute, NSStackView, NSSwitch, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from target object address to closure pointer (f64 NaN-boxed)
    static TOGGLE_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    /// Map from toggle widget handle -> NSSwitch view for two-way binding
    static TOGGLE_SWITCHES: RefCell<HashMap<i64, Retained<NSView>>> = RefCell::new(HashMap::new());
}

// TAG_TRUE and TAG_FALSE from perry-runtime NaN-boxing
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

pub struct PerryToggleTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryToggleTarget"]
    #[ivars = PerryToggleTargetIvars]
    pub struct PerryToggleTarget;

    impl PerryToggleTarget {
        #[unsafe(method(toggleChanged:))]
        fn toggle_changed(&self, sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            crate::catch_callback_panic("toggle callback", std::panic::AssertUnwindSafe(|| {
                TOGGLE_CALLBACKS.with(|cbs| {
                    if let Some(&closure_f64) = cbs.borrow().get(&key) {
                        let state: i64 = unsafe { msg_send![sender, state] };
                        let value = if state != 0 {
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
            }));
        }
    }
);

impl PerryToggleTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryToggleTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Extract a &str from a *const StringHeader pointer.
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

/// Set the on/off state of an existing toggle widget.
/// `on` is 0 for off, non-zero for on.
pub fn set_state(handle: i64, on: i64) {
    TOGGLE_SWITCHES.with(|switches| {
        if let Some(switch_view) = switches.borrow().get(&handle) {
            unsafe {
                let switch: &NSSwitch = &*(Retained::as_ptr(switch_view) as *const NSSwitch);
                let state: i64 = if on != 0 { 1 } else { 0 };
                let _: () = msg_send![switch, setState: state];
            }
        }
    });
}

/// Create an NSSwitch with a label and onChange callback.
/// Returns a widget handle for an HStack containing the label and switch.
pub fn create(label_ptr: *const u8, on_change: f64) -> i64 {
    let label = str_from_header(label_ptr);

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        // Create label
        let ns_label = NSString::from_str(label);
        let text_field = NSTextField::labelWithString(&ns_label, mtm);

        // Create NSSwitch
        let switch = NSSwitch::new(mtm);

        // Create target for action callback
        let target = PerryToggleTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        TOGGLE_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_change);
        });

        let sel = Sel::register(c"toggleChanged:");
        switch.setTarget(Some(&target));
        switch.setAction(Some(sel));

        std::mem::forget(target);

        // Create HStack containing label + switch
        let stack = NSStackView::new(mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        stack.setSpacing(8.0);
        stack.setAlignment(NSLayoutAttribute::CenterY);

        let text_view: Retained<NSView> = Retained::cast_unchecked(text_field);
        let switch_view: Retained<NSView> = Retained::cast_unchecked(switch);

        stack.addArrangedSubview(&text_view);
        stack.addArrangedSubview(&switch_view);

        let view: Retained<NSView> = Retained::cast_unchecked(stack);
        let handle = super::register_widget(view);

        // Store the NSSwitch reference for two-way binding (set_state)
        TOGGLE_SWITCHES.with(|switches| {
            switches.borrow_mut().insert(handle, switch_view.clone());
        });

        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
            perry_geisterhand_register(handle, 3, 1, on_change, label_ptr);
        }

        handle
    }
}
