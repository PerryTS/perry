use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{NSObject, NSString};
use objc2_ui_kit::{UILabel, UIStackView, UISwitch, UIView};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static TOGGLE_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static TOGGLE_SWITCHES: RefCell<HashMap<i64, Retained<UIView>>> = RefCell::new(HashMap::new());
}

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
            TOGGLE_CALLBACKS.with(|cbs| {
                if let Some(&closure_f64) = cbs.borrow().get(&key) {
                    // UISwitch.isOn returns Bool
                    let is_on: bool = unsafe { msg_send![sender, isOn] };
                    let value = if is_on {
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
    TOGGLE_SWITCHES.with(|switches| {
        if let Some(switch_view) = switches.borrow().get(&handle) {
            unsafe {
                let is_on = on != 0;
                let _: () = msg_send![&**switch_view, setOn: is_on, animated: true];
            }
        }
    });
}

/// Create a UISwitch with a label and onChange callback.
/// Returns a widget handle for an HStack containing the label and switch.
pub fn create(label_ptr: *const u8, on_change: f64) -> i64 {
    let label = str_from_header(label_ptr);

    unsafe {
        // Create label
        let ns_label = NSString::from_str(label);
        let text_label: Retained<UILabel> =
            msg_send![objc2::runtime::AnyClass::get(c"UILabel").unwrap(), new];
        let _: () = msg_send![&*text_label, setText: &*ns_label];

        // Create UISwitch
        let switch: Retained<UISwitch> =
            msg_send![objc2::runtime::AnyClass::get(c"UISwitch").unwrap(), new];
        let _: () = msg_send![&*switch, setAccessibilityLabel: &*ns_label];

        let target = PerryToggleTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        TOGGLE_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_change);
        });

        let sel = Sel::register(c"toggleChanged:");
        // UIControlEventValueChanged = 1 << 12 = 4096
        let _: () =
            msg_send![&*switch, addTarget: &*target, action: sel, forControlEvents: 4096u64];

        std::mem::forget(target);

        // Create horizontal UIStackView container
        let stack: Retained<UIStackView> =
            msg_send![objc2::runtime::AnyClass::get(c"UIStackView").unwrap(), new];
        let _: () = msg_send![&*stack, setAxis: 0i64]; // Horizontal
        let _: () = msg_send![&*stack, setSpacing: 8.0f64];
        let _: () = msg_send![&*stack, setAlignment: 3i64]; // Center
        let _: () = msg_send![&*stack, setTranslatesAutoresizingMaskIntoConstraints: false];

        let text_view: Retained<UIView> = Retained::cast_unchecked(text_label);
        let switch_view: Retained<UIView> = Retained::cast_unchecked(switch);

        stack.addArrangedSubview(&text_view);
        stack.addArrangedSubview(&switch_view);

        let view: Retained<UIView> = Retained::cast_unchecked(stack);
        let handle = super::register_widget(view);

        TOGGLE_SWITCHES.with(|switches| {
            switches.borrow_mut().insert(handle, switch_view.clone());
        });

        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
            unsafe {
                perry_geisterhand_register(handle, 3, 1, on_change, label_ptr);
            }
        }

        handle
    }
}
