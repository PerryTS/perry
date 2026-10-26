//! Private writable capabilities. Native lowering receipts consume payload
//! owners; cancellation returns new owned remainder wrappers to the observer.
use super::*;
use crate::object::owned_state_bag::{self as bag, COMPONENT_ASYNC_WRITER};
const OWNER: u32 = 0;
const TOKEN: u32 = 1;
const SCHEMA: u32 = 2;
const STATE: u32 = 3;
const STREAM: u32 = 4;
const PENDING: u32 = 5;
const OPEN: f64 = 1.0;
const CLOSED: f64 = 2.0;
fn fields(wrapper: f64) -> Option<f64> {
    let pointer = object_pointer(wrapper)?;
    let state = unsafe { bag::load(pointer as usize, COMPONENT_ASYNC_WRITER) };
    array_pointer(state).map(|_| state)
}
fn get(state: f64, slot: u32) -> f64 {
    crate::array::js_array_get_f64(array_pointer(state).unwrap(), slot)
}
fn set(state: f64, slot: u32, value: f64) {
    crate::array::js_array_set_f64(array_pointer(state).unwrap(), slot, value);
}
fn descriptor(state: f64) -> Value {
    serde_json::json!({"$writer":string(get(state,TOKEN))})
}

pub(super) fn decode(owner: f64, value: &Value, payload: &Value, stream: bool) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let token = value["$writer"]
        .as_str()
        .filter(|_| value.as_object().is_some_and(|object| object.len() == 1))
        .unwrap_or_else(|| fail("Invalid owned writer"));
    let schema = serde_json::to_vec(payload).unwrap();
    let outcome = crate::exception::catch_js_throw(|| {
        let token = scope.root_nanbox_f64(producer::managed_text(token.as_bytes()));
        let schema = scope.root_nanbox_f64(producer::managed_text(&schema));
        let state = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(6) as i64,
        ));
        for (slot, value) in [
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
            set(state.get_nanbox_f64(), slot as u32, value);
        }
        let wrapper = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        unsafe {
            bag::store(
                object_pointer(wrapper.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_ASYNC_WRITER,
                state.get_nanbox_f64(),
            );
        }
        set_field(wrapper.get_nanbox_f64(), b"then", undefined());
        for (name, info) in [
            (b"write".as_slice(), crate::fn_info!(write, 1)),
            (b"cancel".as_slice(), crate::fn_info!(cancel, 0)),
            (b"dispose".as_slice(), crate::fn_info!(dispose, 0)),
        ] {
            let method = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 1));
            crate::closure::js_closure_set_capture_f64(
                method.get_raw_mut_ptr(),
                0,
                wrapper.get_nanbox_f64(),
            );
            crate::object::set_builtin_closure_non_constructable(
                method.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            );
            set_field(
                wrapper.get_nanbox_f64(),
                name,
                crate::value::js_nanbox_pointer(
                    method.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
                ),
            );
        }
        if stream {
            let close = scope.root_nanbox_f64(field(wrapper.get_nanbox_f64(), b"dispose"));
            set_field(wrapper.get_nanbox_f64(), b"close", close.get_nanbox_f64());
        }
        cleanup::register(
            owner.get_nanbox_f64(),
            wrapper.get_nanbox_f64(),
            serde_json::json!({"kind":"async-writer","writer":value}),
        );
        wrapper.get_nanbox_f64()
    });
    drop(schema);
    finish(outcome)
}

extern "C" fn write(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    input: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let wrapper = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let input = scope.root_nanbox_f64(input);
    let state = scope.root_nanbox_f64(fields(wrapper.get_nanbox_f64()).unwrap());
    if get(state.get_nanbox_f64(), STATE) != OPEN {
        fail("Async writer is closed or consumed");
    }
    if get(state.get_nanbox_f64(), PENDING) != 0.0 {
        fail("Async writer already has a pending write");
    }
    let owner = scope.root_nanbox_f64(get(state.get_nanbox_f64(), OWNER));
    let stream = get(state.get_nanbox_f64(), STREAM) == 1.0;
    let mut payload = Value::Null;
    let mut values = Value::Null;
    let mut writer = Value::Null;
    let mut list = Value::Null;
    set(state.get_nanbox_f64(), PENDING, 1.0);
    let outcome = crate::exception::catch_js_throw(|| {
        payload = serde_json::from_str(&string(get(state.get_nanbox_f64(), SCHEMA)))
            .unwrap_or_else(|_| fail("Invalid writer payload schema"));
        writer = descriptor(state.get_nanbox_f64());
        let retained = scope.root_nanbox_f64(value::retention::new_plan());
        let world = unsafe { &(*context::get(owner.get_nanbox_f64())).world };
        if stream {
            if payload.is_null() {
                let pointer = array_pointer(input.get_nanbox_f64())
                    .unwrap_or_else(|| fail("Unit stream writes require an array"));
                let count = crate::array::js_array_length(pointer);
                if !(1..=65536).contains(&count) {
                    fail("Invalid async write count");
                }
                values = Value::Array(vec![Value::Null; count as usize]);
            } else {
                list = serde_json::json!({"kind":"list","value":payload});
                values = value::typed::result(
                    input.get_nanbox_f64(),
                    &list,
                    world,
                    owner.get_nanbox_f64(),
                    retained.get_nanbox_f64(),
                );
            }
        } else {
            values = Value::Array(vec![value::typed::result(
                input.get_nanbox_f64(),
                &payload,
                world,
                owner.get_nanbox_f64(),
                retained.get_nanbox_f64(),
            )]);
        }
        let count = values
            .as_array()
            .map(Vec::len)
            .unwrap_or_else(|| fail("Async writes require a list"));
        if !(1..=65536).contains(&count) {
            fail("Invalid async write count");
        }
        persistent::operations::write(
            owner.get_nanbox_f64(),
            wrapper.get_nanbox_f64(),
            &values,
            retained.get_nanbox_f64(),
            &payload,
            stream,
            &writer,
        )
    });
    if outcome.is_err() {
        set(state.get_nanbox_f64(), PENDING, 0.0);
    }
    drop((payload, values, writer, list));
    finish(outcome)
}

extern "C" fn cancel(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    control(
        crate::closure::js_closure_get_capture_f64(closure, 0),
        false,
    )
}
extern "C" fn dispose(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    control(crate::closure::js_closure_get_capture_f64(closure, 0), true)
}
fn control(wrapper: f64, close: bool) -> f64 {
    let scope = RuntimeHandleScope::new();
    let wrapper = scope.root_nanbox_f64(wrapper);
    let state = scope.root_nanbox_f64(fields(wrapper.get_nanbox_f64()).unwrap());
    if get(state.get_nanbox_f64(), STATE) == CLOSED {
        return undefined();
    }
    let owner = scope.root_nanbox_f64(get(state.get_nanbox_f64(), OWNER));
    let mut request = Value::Null;
    let outcome = crate::exception::catch_js_throw(|| {
        if !close {
            request = descriptor(state.get_nanbox_f64());
            persistent::operations::cancel(owner.get_nanbox_f64(), &request);
            return undefined();
        }
        request =
            serde_json::json!({"kind":"async-writer","writer":descriptor(state.get_nanbox_f64())});
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
pub(super) fn pending_live(wrapper: f64) -> bool {
    fields(wrapper).is_some_and(|state| get(state, STATE) == OPEN && get(state, PENDING) == 1.0)
}
pub(super) fn finish_write(owner: f64, target: f64, closed: bool) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let target = scope.root_nanbox_f64(crate::weakref::js_weakref_deref(target));
    if let Some(state) = fields(target.get_nanbox_f64()) {
        set(state, PENDING, 0.0);
        if closed {
            set(state, STATE, CLOSED);
            cleanup::unregister(owner.get_nanbox_f64(), target.get_nanbox_f64());
        }
    }
}
