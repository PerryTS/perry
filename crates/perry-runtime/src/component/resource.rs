//! Resource capabilities are private object-owned state, never public tokens.
//! The engine remains authoritative for ownership, type and call lifetimes.
use super::*;
use crate::object::owned_state_bag::{
    self as bag, COMPONENT_RESOURCE_BORROW as BORROW, COMPONENT_RESOURCE_OWNER as OWNER,
    COMPONENT_RESOURCE_PARENT as PARENT, COMPONENT_RESOURCE_STATE as STATE,
    COMPONENT_RESOURCE_TOKEN as TOKEN,
};

fn load(resource: f64, slot: u32) -> f64 {
    unsafe {
        bag::load(
            object_pointer(resource).unwrap_or_else(|| fail("Expected a component resource"))
                as usize,
            slot,
        )
    }
}

fn store(resource: f64, slot: u32, value: f64) {
    unsafe {
        bag::store(object_pointer(resource).unwrap() as usize, slot, value);
    }
}

pub(super) fn encode(resource: f64) -> Option<Value> {
    object_pointer(resource)?;
    let token = load(resource, TOKEN);
    if JSValue::from_bits(token.to_bits()).is_undefined() {
        return None;
    }
    let state = load(resource, STATE);
    if state == 2.0 {
        fail("Component resource is disposed");
    }
    let borrow = load(resource, BORROW);
    if !JSValue::from_bits(borrow.to_bits()).is_undefined() {
        let borrow = array_pointer(borrow).unwrap();
        let lifetime = crate::array::js_array_get_f64(borrow, 1);
        if !JSValue::from_bits(lifetime.to_bits()).is_undefined()
            && crate::array::js_array_get_f64(array_pointer(lifetime).unwrap(), 0) != 1.0
        {
            fail("Component resource borrow has expired");
        }
    }
    let mut value = serde_json::json!({"$resource":string(token), "owned":state == 1.0});
    if !JSValue::from_bits(borrow.to_bits()).is_undefined() {
        value["$borrow"] = Value::String(string(crate::array::js_array_get_f64(
            array_pointer(borrow).unwrap(),
            0,
        )));
    }
    Some(value)
}

pub(super) fn decode(owner: f64, value: &Value, lifetime: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let lifetime = scope.root_nanbox_f64(lifetime);
    let token = value["$resource"]
        .as_str()
        .unwrap_or_else(|| fail("Invalid resource token"));
    let owned = value["owned"] == true;
    if let Some(capability) = value.get("$borrow") {
        if owned || capability.as_str() != Some(token) {
            fail("Invalid component resource borrow capability");
        }
    }
    let resource = scope.root_nanbox_f64(wrap(owner.get_nanbox_f64(), token, owned));
    if value.get("$borrow").is_some() {
        let state = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(2));
        crate::array::js_array_set_f64(
            state.get_raw_mut_ptr(),
            0,
            load(resource.get_nanbox_f64(), TOKEN),
        );
        crate::array::js_array_set_f64(state.get_raw_mut_ptr(), 1, lifetime.get_nanbox_f64());
        store(
            resource.get_nanbox_f64(),
            BORROW,
            crate::value::js_nanbox_pointer(
                state.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64
            ),
        );
    }
    resource.get_nanbox_f64()
}

pub(super) fn wrap(owner: f64, token: &str, owned: bool) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let resource = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let token = scope.root_nanbox_f64(crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(token.as_ptr(), token.len() as u32) as i64,
    ));
    store(resource.get_nanbox_f64(), OWNER, owner.get_nanbox_f64());
    store(resource.get_nanbox_f64(), TOKEN, token.get_nanbox_f64());
    store(
        resource.get_nanbox_f64(),
        STATE,
        if owned { 1.0 } else { 0.0 },
    );
    if owned {
        for (name, info) in [
            ("borrow", crate::fn_info!(borrow, 0)),
            ("dispose", crate::fn_info!(dispose, 0)),
        ] {
            let function = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 1));
            crate::closure::js_closure_set_capture_f64(
                function.get_raw_mut_ptr(),
                0,
                resource.get_nanbox_f64(),
            );
            crate::object::set_builtin_closure_non_constructable(
                function.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            );
            let value = crate::value::js_nanbox_pointer(
                function.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            );
            set_field(resource.get_nanbox_f64(), name.as_bytes(), value);
        }
        cleanup::register(
            owner.get_nanbox_f64(),
            resource.get_nanbox_f64(),
            serde_json::json!({"kind":"guest", "token":string(token.get_nanbox_f64())}),
        );
    }
    resource.get_nanbox_f64()
}

extern "C" fn borrow(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    if load(resource.get_nanbox_f64(), STATE) != 1.0 {
        fail("Component resource is not owned");
    }
    let token = string(load(resource.get_nanbox_f64(), TOKEN));
    let borrowed =
        scope.root_nanbox_f64(wrap(load(resource.get_nanbox_f64(), OWNER), &token, false));
    store(borrowed.get_nanbox_f64(), PARENT, resource.get_nanbox_f64());
    borrowed.get_nanbox_f64()
}

extern "C" fn dispose(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let state = load(resource.get_nanbox_f64(), STATE);
    if state == 2.0 {
        return undefined();
    }
    if state != 1.0 {
        fail("A borrowed component resource cannot be disposed");
    }
    let frame = callback::Frame::new(&scope, load(resource.get_nanbox_f64(), OWNER));
    let context = context::get(frame.owner.get_nanbox_f64());
    let mut token = String::new();
    let mut error = Vec::new();
    let result = crate::exception::catch_js_throw(|| {
        token = string(load(resource.get_nanbox_f64(), TOKEN));
        if persistent::enabled(frame.owner.get_nanbox_f64()) {
            let request =
                serde_json::json!({"op":"resource-drop","value":{"$resource":token,"owned":true}});
            let result = crate::exception::catch_js_throw(|| {
                persistent::submit::unit(frame.owner.get_nanbox_f64(), &request)
            });
            drop(request);
            if result.is_ok() || !unsafe { (*context).alive } {
                store(resource.get_nanbox_f64(), STATE, 2.0);
                cleanup::unregister(frame.owner.get_nanbox_f64(), resource.get_nanbox_f64());
            }
            return finish(result);
        }
        let mut buffer = Buffer::default();
        let status = unsafe {
            (host::ops().perry_component_host_resource_drop)(
                (*context).host,
                token.as_ptr(),
                token.len(),
                frame.token(),
                &mut buffer,
            )
        };
        error = unsafe { take(buffer) };
        if matches!(
            status,
            perry_abi::component_host::OK | perry_abi::component_host::RELEASED_ERROR
        ) {
            store(resource.get_nanbox_f64(), STATE, 2.0);
            cleanup::unregister(frame.owner.get_nanbox_f64(), resource.get_nanbox_f64());
        }
        frame.rethrow();
        if status != perry_abi::component_host::OK {
            fail(&String::from_utf8_lossy(&error));
        }
        undefined()
    });
    drop((token, error));
    cleanup::finish_call(frame.owner.get_nanbox_f64(), result)
}
