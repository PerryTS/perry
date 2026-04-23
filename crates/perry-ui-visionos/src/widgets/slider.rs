use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_ui_kit::{UISlider, UIView};
use objc2_foundation::NSObject;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static SLIDER_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

pub struct PerrySliderTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerrySliderTarget"]
    #[ivars = PerrySliderTargetIvars]
    pub struct PerrySliderTarget;

    impl PerrySliderTarget {
        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            SLIDER_CALLBACKS.with(|cbs| {
                if let Some(&closure_f64) = cbs.borrow().get(&key) {
                    // UISlider uses Float (f32), cast to f64 at boundary
                    let value: f32 = unsafe { msg_send![sender, value] };
                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call1(closure_ptr as *const u8, value as f64);
                    }
                }
            });
        }
    }
);

impl PerrySliderTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerrySliderTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Set the value of an existing UISlider widget.
pub fn set_value(handle: i64, value: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, setValue: value as f32, animated: true];
        }
    }
}

/// Create a UISlider with min, max, initial values and onChange callback.
pub fn create(min: f64, max: f64, initial: f64, on_change: f64) -> i64 {
    unsafe {
        let slider: Retained<UISlider> = msg_send![
            objc2::runtime::AnyClass::get(c"UISlider").unwrap(),
            new
        ];
        let _: () = msg_send![&*slider, setMinimumValue: min as f32];
        let _: () = msg_send![&*slider, setMaximumValue: max as f32];
        let _: () = msg_send![&*slider, setValue: initial as f32];
        let _: () = msg_send![&*slider, setContinuous: true];
        let _: () = msg_send![&*slider, setTranslatesAutoresizingMaskIntoConstraints: false];

        let target = PerrySliderTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        SLIDER_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_change);
        });

        let sel = Sel::register(c"sliderChanged:");
        // UIControlEventValueChanged = 4096
        let _: () = msg_send![&*slider, addTarget: &*target, action: sel, forControlEvents: 4096u64];

        std::mem::forget(target);

        let view: Retained<UIView> = Retained::cast_unchecked(slider);
        let handle = super::register_widget(view);
        #[cfg(feature = "geisterhand")]
        {
            extern "C" { fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8); }
            unsafe { perry_geisterhand_register(handle, 2, 1, on_change, std::ptr::null()); }
        }
        handle
    }
}
