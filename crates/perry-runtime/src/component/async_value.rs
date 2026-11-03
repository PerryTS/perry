//! Private, owned readable capabilities. Pending operations hold weak targets;
//! their public Promise owns the handle until completion or abandonment.
use super::*;
use crate::object::owned_state_bag::{self as bag, COMPONENT_ASYNC_VALUE};
const OWNER: u32 = 0;
const TOKEN: u32 = 1;
const SCHEMA: u32 = 2;
const STATE: u32 = 3;
const STREAM: u32 = 4;
const PENDING: u32 = 5;
const OPEN: f64 = 1.0;
const CLOSED: f64 = 2.0;
fn fields(value: f64) -> Option<f64> {
    let pointer = object_pointer(value)?;
    let fields = unsafe { bag::load(pointer as usize, COMPONENT_ASYNC_VALUE) };
    array_pointer(fields).map(|_| fields)
}
fn get(fields: f64, index: u32) -> f64 {
    crate::array::js_array_get_f64(array_pointer(fields).unwrap(), index)
}
fn set(fields: f64, index: u32, value: f64) {
    crate::array::js_array_set_f64(array_pointer(fields).unwrap(), index, value);
}
fn text(bytes: &[u8]) -> f64 {
    crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ) as i64)
}
fn descriptor(fields: f64) -> Value {
    let token = string(get(fields, TOKEN));
    if get(fields, STREAM) == 1.0 {
        serde_json::json!({"$stream":token})
    } else {
        serde_json::json!({"$future":token})
    }
}
pub(super) fn encode(value: f64, owner: f64) -> Option<Value> {
    let fields = fields(value)?;
    if get(fields, OWNER).to_bits() != owner.to_bits() {
        fail("Async value belongs to another component");
    }
    if get(fields, STATE) != OPEN {
        fail("Async value is closed or consumed");
    }
    if get(fields, PENDING) != 0.0 {
        fail("Cannot transfer an async value with a pending read");
    }
    if !unsafe { (*context::get(owner)).alive } {
        fail("Component has been disposed");
    }
    Some(descriptor(fields))
}
pub(super) fn decode(owner: f64, value: &Value, payload: &Value, stream: bool) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    if !persistent::enabled(owner.get_nanbox_f64()) {
        fail("Async values require the persistent component driver");
    }
    let key = if stream { "$stream" } else { "$future" };
    let token = value[key]
        .as_str()
        .filter(|_| value.as_object().is_some_and(|o| o.len() == 1))
        .unwrap_or_else(|| fail("Invalid owned async value"));
    let schema = serde_json::to_vec(payload).unwrap();
    let outcome = crate::exception::catch_js_throw(|| {
        let token = scope.root_nanbox_f64(text(token.as_bytes()));
        let schema = scope.root_nanbox_f64(text(&schema));
        let state = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(6) as i64,
        ));
        for (index, value) in [
            owner.get_nanbox_f64(),
            token.get_nanbox_f64(),
            schema.get_nanbox_f64(),
            OPEN,
            if stream { 1.0 } else { 0.0 },
            0.0,
        ]
        .into_iter()
        .enumerate()
        {
            set(state.get_nanbox_f64(), index as u32, value);
        }
        let wrapper = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        unsafe {
            bag::store(
                object_pointer(wrapper.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_ASYNC_VALUE,
                state.get_nanbox_f64(),
            );
        }
        set_field(wrapper.get_nanbox_f64(), b"then", undefined());
        for (name, info) in [
            (
                b"read".as_slice(),
                if stream {
                    crate::fn_info!(stream_read, 1)
                } else {
                    crate::fn_info!(future_read, 0)
                },
            ),
            (b"dispose".as_slice(), crate::fn_info!(dispose, 0)),
        ] {
            let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 1));
            crate::closure::js_closure_set_capture_f64(
                closure.get_raw_mut_ptr(),
                0,
                wrapper.get_nanbox_f64(),
            );
            crate::object::set_builtin_closure_non_constructable(
                closure.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            );
            set_field(
                wrapper.get_nanbox_f64(),
                name,
                crate::value::js_nanbox_pointer(
                    closure.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
                ),
            );
        }
        cleanup::register(
            owner.get_nanbox_f64(),
            wrapper.get_nanbox_f64(),
            serde_json::json!({"kind":"async-value","value":value}),
        );
        wrapper.get_nanbox_f64()
    });
    drop(schema);
    finish(outcome)
}
extern "C" fn future_read(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    begin_read(
        crate::closure::js_closure_get_capture_f64(closure, 0),
        undefined(),
    )
}
extern "C" fn stream_read(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    capacity: f64,
) -> f64 {
    begin_read(
        crate::closure::js_closure_get_capture_f64(closure, 0),
        capacity,
    )
}
fn begin_read(wrapper: f64, capacity: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let wrapper = scope.root_nanbox_f64(wrapper);
    let state = scope.root_nanbox_f64(fields(wrapper.get_nanbox_f64()).unwrap());
    let stream = get(state.get_nanbox_f64(), STREAM) == 1.0;
    if get(state.get_nanbox_f64(), STATE) != OPEN {
        fail("Async value is closed or consumed");
    }
    if get(state.get_nanbox_f64(), PENDING) != 0.0 {
        fail("Async value already has a pending read");
    }
    let capacity = if JSValue::from_bits(capacity.to_bits()).is_undefined() {
        if stream {
            4096.0
        } else {
            1.0
        }
    } else {
        capacity
    };
    if !JSValue::from_bits(capacity.to_bits()).is_number()
        || capacity.fract() != 0.0
        || !(1.0..=65536.0).contains(&capacity)
        || (!stream && capacity != 1.0)
    {
        fail("Invalid async read capacity");
    }
    let mut payload = Value::Null;
    let mut value = Value::Null;
    set(state.get_nanbox_f64(), PENDING, 1.0);
    let outcome = crate::exception::catch_js_throw(|| {
        payload = serde_json::from_str(&string(get(state.get_nanbox_f64(), SCHEMA)))
            .unwrap_or_else(|_| fail("Invalid async payload schema"));
        value = descriptor(state.get_nanbox_f64());
        persistent::submit::read(
            get(state.get_nanbox_f64(), OWNER),
            wrapper.get_nanbox_f64(),
            &value,
            &payload,
            stream,
            capacity as u32,
        )
    });
    if outcome.is_err() {
        set(state.get_nanbox_f64(), PENDING, 0.0);
    }
    drop((payload, value));
    finish(outcome)
}
extern "C" fn dispose(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let wrapper = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let state = scope.root_nanbox_f64(fields(wrapper.get_nanbox_f64()).unwrap());
    if get(state.get_nanbox_f64(), STATE) == CLOSED {
        return undefined();
    }
    let owner = scope.root_nanbox_f64(get(state.get_nanbox_f64(), OWNER));
    let mut request = Value::Null;
    let outcome = crate::exception::catch_js_throw(|| {
        request =
            serde_json::json!({"kind":"async-value","value":descriptor(state.get_nanbox_f64())});
        set(state.get_nanbox_f64(), STATE, CLOSED);
        cleanup::unregister(owner.get_nanbox_f64(), wrapper.get_nanbox_f64());
        let ctx = context::get(owner.get_nanbox_f64());
        if unsafe { (*ctx).alive } {
            unsafe {
                (*ctx)
                    .pending_collections
                    .push(std::mem::take(&mut request));
            }
            cleanup::drain(owner.get_nanbox_f64());
        }
        undefined()
    });
    drop(request);
    finish(outcome)
}
pub(super) fn finish_read(owner: f64, target: f64, stream: bool, closed: bool) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let target = scope.root_nanbox_f64(crate::weakref::js_weakref_deref(target));
    if let Some(state) = fields(target.get_nanbox_f64()) {
        set(state, PENDING, 0.0);
        if !stream || closed {
            set(state, STATE, CLOSED);
            cleanup::unregister(owner.get_nanbox_f64(), target.get_nanbox_f64());
        }
    }
}

pub(super) fn pending_read_live(wrapper: f64) -> bool {
    fields(wrapper).is_some_and(|state| get(state, STATE) == OPEN && get(state, PENDING) == 1.0)
}

pub(super) fn transferred(wrapper: f64) {
    if let Some(state) = fields(wrapper) {
        if get(state, STATE) == OPEN {
            set(state, STATE, CLOSED);
            cleanup::unregister(get(state, OWNER), wrapper);
        }
    }
}

/// A void stream carries counts rather than payload bytes. Materialize one
/// undefined element per unit; ordinary byte streams retain Uint8Array chunks.
pub(super) fn chunk(
    owner: f64,
    input: &Value,
    payload: &Value,
    world: &Value,
    stream: bool,
) -> f64 {
    if payload.is_null() {
        let count = input
            .as_array()
            .unwrap_or_else(|| fail("Invalid unit chunk"))
            .len();
        let count: u32 = count
            .try_into()
            .unwrap_or_else(|_| fail("Async chunk is too large"));
        let scope = RuntimeHandleScope::new();
        let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(count));
        for index in 0..count {
            crate::array::js_array_set_f64(array.get_raw_mut_ptr(), index, undefined());
        }
        return crate::value::js_nanbox_pointer(
            array.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
        );
    }
    let schema = if stream {
        serde_json::json!({"kind":"list","value":payload})
    } else {
        serde_json::json!({"kind":"tuple","value":vec![payload;input.as_array().unwrap_or_else(||fail("Invalid write remainder")).len()]})
    };
    let outcome = crate::exception::catch_js_throw(|| value::decode(input, &schema, world, owner));
    drop(schema);
    finish(outcome)
}
