//! Native host resource wrappers retain private owners, generations and borrow
//! lifetimes. Only the payload table is rooted while a guest owns a resource.
use super::*;
use crate::object::owned_state_bag::{self as bag, COMPONENT_HOST_RESOURCE};

#[path = "host_payload.rs"]
pub(super) mod payload;

const OWNER: u32 = 0;
const ID: u32 = 1;
const GENERATION: u32 = 2;
const STATE: u32 = 3;
const LIFETIME: u32 = 4;
const PARENT: u32 = 5;
const BORROW: u32 = 6;
const OWNED: f64 = 1.0;
const BORROWED: f64 = 0.0;
const DISPOSED: f64 = 2.0;

fn array(value: f64) -> *mut crate::array::ArrayHeader {
    array_pointer(value).unwrap_or_else(|| fail("Invalid host resource state"))
}
fn get(value: f64, index: u32) -> f64 {
    crate::array::js_array_get_f64(array(value), index)
}
fn set(value: f64, index: u32, item: f64) {
    crate::array::js_array_set_f64(array(value), index, item);
}
fn fields(resource: f64) -> f64 {
    let pointer = object_pointer(resource).unwrap_or_else(|| fail("Expected a host resource"));
    let value = unsafe { bag::load(pointer as usize, COMPONENT_HOST_RESOURCE) };
    array(value);
    value
}
fn live(resource: f64) -> (f64, f64) {
    let fields = fields(resource);
    if get(fields, STATE) == DISPOSED {
        fail("Host resource is disposed");
    }
    let owner = get(fields, OWNER);
    if !unsafe { (*context::get(owner)).alive } {
        fail("Component is disposed");
    }
    let record = payload::record(owner, get(fields, ID));
    if get(record, payload::GENERATION) != get(fields, GENERATION) {
        fail("Host resource owner is stale");
    }
    if get(fields, STATE) == OWNED && get(record, payload::STATE) != payload::LOCAL {
        fail("Host resource was transferred to the guest");
    }
    let lifetime = get(fields, LIFETIME);
    if get(fields, STATE) == BORROWED
        && !JSValue::from_bits(lifetime.to_bits()).is_undefined()
        && get(lifetime, 0) != 1.0
    {
        fail("Host resource borrow has expired");
    }
    let parent = get(fields, PARENT);
    if !JSValue::from_bits(parent.to_bits()).is_undefined() {
        live(parent);
    }
    (fields, record)
}

fn wrap(
    owner: f64,
    id: u32,
    generation: u32,
    owned: bool,
    lifetime: f64,
    parent: f64,
    capability: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let lifetime = scope.root_nanbox_f64(lifetime);
    let parent = scope.root_nanbox_f64(parent);
    let capability = scope.root_nanbox_f64(capability);
    let fields = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(7) as i64,
    ));
    for (index, value) in [
        (OWNER, owner.get_nanbox_f64()),
        (ID, id as f64),
        (GENERATION, generation as f64),
        (STATE, if owned { OWNED } else { BORROWED }),
        (LIFETIME, lifetime.get_nanbox_f64()),
        (PARENT, parent.get_nanbox_f64()),
        (BORROW, capability.get_nanbox_f64()),
    ] {
        set(fields.get_nanbox_f64(), index, value);
    }
    let wrapper = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    unsafe {
        bag::store(
            object_pointer(wrapper.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_HOST_RESOURCE,
            fields.get_nanbox_f64(),
        );
    }
    if owned {
        for (name, info) in [
            ("borrow", crate::fn_info!(borrow, 0)),
            ("dispose", crate::fn_info!(dispose, 0)),
        ] {
            let function = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 1));
            crate::closure::js_closure_set_capture_f64(
                function.get_raw_mut_ptr(),
                0,
                wrapper.get_nanbox_f64(),
            );
            crate::object::set_builtin_closure_non_constructable(
                function.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            );
            set_field(
                wrapper.get_nanbox_f64(),
                name.as_bytes(),
                crate::value::js_nanbox_pointer(
                    function.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
                ),
            );
        }
        let row = payload::record(owner.get_nanbox_f64(), id as f64);
        cleanup::register(
            owner.get_nanbox_f64(),
            wrapper.get_nanbox_f64(),
            serde_json::json!({
                "kind":"host", "resource":{"token":id.to_string(),
                    "type":string(get(row, payload::TYPE)), "generation":generation}
            }),
        );
    }
    wrapper.get_nanbox_f64()
}

pub(super) fn encode(resource: f64, owner: f64) -> Option<Value> {
    let pointer = object_pointer(resource)?;
    let private = unsafe { bag::load(pointer as usize, COMPONENT_HOST_RESOURCE) };
    if JSValue::from_bits(private.to_bits()).is_undefined() {
        return None;
    }
    let (fields, record) = live(resource);
    if get(fields, OWNER).to_bits() != owner.to_bits() {
        fail("Host resource belongs to another component");
    }
    let mut value = serde_json::json!({"$hostResource":(get(fields, ID) as u32).to_string(),
        "type":string(get(record, payload::TYPE)),"generation":get(fields, GENERATION) as u32,
        "owned":get(fields, STATE) == OWNED,"new":get(record, payload::MATERIALIZED) == 0.0});
    let borrow = get(fields, BORROW);
    if !JSValue::from_bits(borrow.to_bits()).is_undefined() {
        value["$borrow"] = Value::String(string(borrow));
    }
    Some(value)
}

pub(super) fn decode(owner: f64, value: &Value, lifetime: f64) -> f64 {
    let id = value["$hostResource"]
        .as_str()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or_else(|| fail("Invalid host resource identity"));
    let record = payload::record(owner, id as f64);
    let generation = value["generation"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or_else(|| fail("Invalid host resource generation"));
    if get(record, payload::GENERATION) != generation as f64
        || Some(string(get(record, payload::TYPE)).as_str()) != value["type"].as_str()
    {
        fail("Host resource identity does not match its payload");
    }
    let owned = value["owned"] == true;
    if owned && get(record, payload::STATE) != payload::LOCAL {
        fail("Host resource ownership was not returned");
    }
    if !owned && JSValue::from_bits(lifetime.to_bits()).is_undefined() {
        fail("A host resource borrow needs a live callback");
    }
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let lifetime = scope.root_nanbox_f64(lifetime);
    let borrow = match value.get("$borrow") {
        Some(Value::String(token)) if !owned => crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(token.as_ptr(), token.len() as u32) as i64,
        ),
        None => undefined(),
        _ => fail("Invalid host resource borrow capability"),
    };
    wrap(
        owner.get_nanbox_f64(),
        id,
        generation,
        owned,
        lifetime.get_nanbox_f64(),
        undefined(),
        borrow,
    )
}

extern "C" fn borrow(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let (fields, _) = live(resource.get_nanbox_f64());
    if get(fields, STATE) != OWNED {
        fail("Only an owned host resource can be borrowed");
    }
    wrap(
        get(fields, OWNER),
        get(fields, ID) as u32,
        get(fields, GENERATION) as u32,
        false,
        undefined(),
        resource.get_nanbox_f64(),
        undefined(),
    )
}

extern "C" fn dispose(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let fields = fields(resource.get_nanbox_f64());
    if get(fields, STATE) == DISPOSED {
        return undefined();
    }
    let frame = callback::Frame::new(&scope, get(fields, OWNER));
    let mut token = String::new();
    let mut error = Vec::new();
    let result = crate::exception::catch_js_throw(|| {
        let (fields, record) = live(resource.get_nanbox_f64());
        if get(fields, STATE) != OWNED {
            fail("A borrowed host resource cannot be disposed");
        }
        let id = get(fields, ID);
        if get(record, payload::MATERIALIZED) == 0.0 {
            set(fields, STATE, DISPOSED);
            cleanup::unregister(frame.owner.get_nanbox_f64(), resource.get_nanbox_f64());
            payload::destroy(frame.owner.get_nanbox_f64(), id);
            return undefined();
        }
        let lifetime = get(fields, LIFETIME);
        if !JSValue::from_bits(lifetime.to_bits()).is_undefined() && get(lifetime, 0) == 1.0 {
            // This owner arrived as an owned import argument. Its canonical
            // handle is retired immediately after the callback returns. An
            // existing owner borrowed by the active call has no such origin
            // and must still pass through the engine's reentry guard.
            set(fields, STATE, DISPOSED);
            cleanup::unregister(frame.owner.get_nanbox_f64(), resource.get_nanbox_f64());
            payload::retire(frame.owner.get_nanbox_f64(), id);
            return undefined();
        }
        token = (id as u32).to_string();
        let context = context::get(frame.owner.get_nanbox_f64());
        if persistent::enabled(frame.owner.get_nanbox_f64()) {
            let request = serde_json::json!({"op":"resource-drop","value":{
                "$hostResource":token,"owned":true,"new":false,
                "type":string(get(record, payload::TYPE)),"generation":get(record,payload::GENERATION) as u32
            }});
            let result = crate::exception::catch_js_throw(|| {
                persistent::submit::unit(frame.owner.get_nanbox_f64(), &request)
            });
            drop(request);
            if result.is_ok() || !unsafe { (*context).alive } {
                set(self::fields(resource.get_nanbox_f64()), STATE, DISPOSED);
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
            set(self::fields(resource.get_nanbox_f64()), STATE, DISPOSED);
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

#[no_mangle]
pub extern "C" fn js_component_host_resource_value(owner: f64, ty: f64, resource: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let ty = scope.root_nanbox_f64(ty);
    let resource = scope.root_nanbox_f64(resource);
    let mut expected = String::new();
    let mut actual = String::new();
    let result = crate::exception::catch_js_throw(|| {
        expected = string(ty.get_nanbox_f64());
        let (fields, record) = live(resource.get_nanbox_f64());
        actual = string(get(record, payload::TYPE));
        if get(fields, OWNER).to_bits() != owner.get_nanbox_f64().to_bits() || actual != expected {
            fail("Expected a host resource of this instance and type");
        }
        get(record, payload::PAYLOAD)
    });
    drop((expected, actual));
    finish(result)
}
