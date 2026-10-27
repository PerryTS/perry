//! HTTP's existing Promise and native accept join the ordinary origin table.
use super::*;

pub(in crate::component) fn profile(wasm: &[u8], contract: &[u8]) -> bool {
    let mut buffer = Buffer::default();
    let status = unsafe {
        (host::ops().perry_component_driver_http_profile)(
            wasm.as_ptr(),
            wasm.len(),
            contract.as_ptr(),
            contract.len(),
            &mut buffer,
        )
    };
    let output = unsafe { take(buffer) };
    let parsed: Result<Value, _> = serde_json::from_slice(&output);
    let concurrent = parsed
        .as_ref()
        .ok()
        .and_then(|value| value["concurrent"].as_bool());
    if status != OK || concurrent.is_none() {
        let message = if status == OK {
            "Invalid component HTTP profile".to_owned()
        } else {
            String::from_utf8_lossy(&output).into_owned()
        };
        drop((output, parsed));
        let outcome = crate::exception::catch_js_throw(|| fail(&message));
        drop(message);
        finish(outcome);
        unreachable!();
    }
    concurrent.unwrap()
}

pub(in crate::component) fn submit(
    component: f64,
    interface: &str,
    http_owner: f64,
    request: *mut core::ffi::c_void,
    promise: f64,
) {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let http_owner = scope.root_nanbox_f64(http_owner);
    let promise = scope.root_nanbox_f64(promise);
    submit::available(component.get_nanbox_f64(), false);
    let (saved, ids) = callback::saved::capture(promise.get_nanbox_f64());
    let saved = scope.root_nanbox_f64(saved);
    let row = scope.root_nanbox_f64(records::create(
        promise.get_nanbox_f64(),
        http_owner.get_nanbox_f64(),
        saved.get_nanbox_f64(),
    ));
    submit::admit_native(
        component.get_nanbox_f64(),
        row.get_nanbox_f64(),
        ids,
        &ReplyKind::Unit,
        false,
        |handle, buffer| unsafe {
            (host::ops().perry_component_driver_http_submit)(
                handle,
                request,
                interface.as_ptr(),
                interface.len(),
                buffer,
            )
        },
    );
}
