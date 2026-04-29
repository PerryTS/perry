use gtk4::prelude::*;
use gtk4::{Adjustment, Orientation, Scale};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from slider ID to closure pointer (f64 NaN-boxed)
    static SLIDER_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static NEXT_SLIDER_ID: RefCell<usize> = RefCell::new(1);
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Set the value of an existing slider widget.
pub fn set_value(handle: i64, value: f64) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(scale) = widget.downcast_ref::<Scale>() {
            scale.set_value(value);
        }
    }
}

/// Create a horizontal GtkScale with min, max, initial values and onChange callback.
pub fn create(min: f64, max: f64, initial: f64, on_change: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let adjustment = Adjustment::new(initial, min, max, 1.0, 10.0, 0.0);
    let scale = Scale::new(Orientation::Horizontal, Some(&adjustment));
    scale.set_draw_value(false);
    scale.set_hexpand(true);

    let callback_id = NEXT_SLIDER_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    SLIDER_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(callback_id, on_change);
    });

    scale.connect_value_changed(move |scale| {
        let closure_f64 = SLIDER_CALLBACKS.with(|cbs| cbs.borrow().get(&callback_id).copied());
        if let Some(closure_f64) = closure_f64 {
            let value = scale.value();
            let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
            unsafe {
                js_closure_call1(closure_ptr as *const u8, value);
            }
        }
    });

    super::register_widget(scale.upcast())
}
