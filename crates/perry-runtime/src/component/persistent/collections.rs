//! Collector requests remain native identities until admission succeeds. Queue
//! pressure defers cleanup instead of losing a collected resource permanently.
use super::*;
pub(in crate::component) fn drain_collections(component: f64) {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let mut request = Value::Null;
    let outcome = crate::exception::catch_js_throw(|| loop {
        let ctx = context::get(component.get_nanbox_f64());
        if unsafe { !(*ctx).alive || (*ctx).host.is_null() } {
            break;
        }
        if !submit::has_capacity(component.get_nanbox_f64()) {
            break;
        }
        let next = unsafe { (*ctx).pending_collections.pop() };
        let Some(next) = next else {
            break;
        };
        request = next;
        submit::collection(component.get_nanbox_f64(), &request);
        request = Value::Null;
    });
    if outcome.is_err() && !request.is_null() {
        unsafe {
            (*context::get(component.get_nanbox_f64()))
                .pending_collections
                .push(std::mem::take(&mut request));
        }
    }
    drop(request);
    if let Err(error) = outcome {
        crate::exception::js_throw(error);
    }
}
