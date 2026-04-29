use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{NSSlider, NSView};
use objc2_foundation::{MainThreadMarker, NSObject};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from target object address to closure pointer (f64 NaN-boxed)
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
            crate::catch_callback_panic("slider callback", std::panic::AssertUnwindSafe(|| {
                SLIDER_CALLBACKS.with(|cbs| {
                    if let Some(&closure_f64) = cbs.borrow().get(&key) {
                        let value: f64 = unsafe { msg_send![sender, doubleValue] };

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

impl PerrySliderTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerrySliderTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Set the value of an existing slider widget.
pub fn set_value(handle: i64, value: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let slider: &NSSlider = &*(Retained::as_ptr(&view) as *const NSSlider);
            slider.setDoubleValue(value);
        }
    }
}

/// Create a horizontal NSSlider with min, max, initial values and onChange callback.
/// `on_change` is a NaN-boxed closure pointer, called with the slider's f64 value.
pub fn create(min: f64, max: f64, initial: f64, on_change: f64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let slider = NSSlider::new(mtm);
        slider.setMinValue(min);
        slider.setMaxValue(max);
        slider.setDoubleValue(initial);

        // Continuous mode: fire callback while dragging
        slider.setContinuous(true);

        // Set up target-action
        let target = PerrySliderTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        SLIDER_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_change);
        });

        let sel = Sel::register(c"sliderChanged:");
        slider.setTarget(Some(&target));
        slider.setAction(Some(sel));

        std::mem::forget(target);

        let view: Retained<NSView> = Retained::cast_unchecked(slider);
        let handle = super::register_widget(view);

        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
            perry_geisterhand_register(handle, 2, 1, on_change, std::ptr::null());
        }

        handle
    }
}
