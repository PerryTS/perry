//! Admission never enters Wasm. Publish traced arguments/context before the
//! native queue can first observe the request on a later poll.
use super::*;
use crate::async_hooks::AsyncResourceIds;
const MAX_CONTEXTS: usize = 1024;

pub(super) fn available(component: f64, synchronous: bool) {
    let ctx = context::get(component);
    if unsafe { !(*ctx).alive || (*ctx).host.is_null() || (*state(component)).retired } {
        fail("Component is not initialized or has been disposed");
    }
    if synchronous && unsafe { (*state(component)).polling || (*state(component)).cycling } {
        fail("Cannot recursively execute a synchronous component operation");
    }
    if !has_capacity(component) {
        fail("Component invocation limit exceeded");
    }
}

pub(super) fn admit(
    component: f64,
    request: &[u8],
    row: f64,
    ids: AsyncResourceIds,
    reply: &ReplyKind,
    synchronous: bool,
) -> u64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let row = scope.root_nanbox_f64(row);
    available(component.get_nanbox_f64(), synchronous);
    let slot = unsafe {
        let data = &mut *state(component.get_nanbox_f64());
        data.free.pop().unwrap_or_else(|| {
            let slot = data.slots;
            data.slots += 1;
            slot
        })
    };
    records::insert(
        owner(component.get_nanbox_f64()),
        slot,
        row.get_nanbox_f64(),
    );
    // There are no JS calls between native acceptance and installing metadata.
    let mut buffer = Buffer::default();
    let status = unsafe {
        (host::ops().perry_component_driver_submit)(
            (*context::get(component.get_nanbox_f64())).host,
            request.as_ptr(),
            request.len(),
            &mut buffer,
        )
    };
    let output = unsafe { take(buffer) };
    let parsed: Result<Value, _> = serde_json::from_slice(&output);
    let id = parsed
        .as_ref()
        .ok()
        .and_then(|value| value["id"].as_str())
        .and_then(|id| id.parse::<u64>().ok())
        .filter(|id| *id != 0);
    if status != OK || id.is_none() {
        records::release(records::table(owner(component.get_nanbox_f64())), slot);
        unsafe {
            (*state(component.get_nanbox_f64())).free.push(slot);
        }
        // Destroy native request/reply owners before a JS throw abandons us.
        let message = if status == OK {
            "Invalid persistent submission result".to_owned()
        } else {
            String::from_utf8_lossy(&output).into_owned()
        };
        drop((output, parsed));
        let outcome = crate::exception::catch_js_throw(|| fail(&message));
        drop(message);
        finish(outcome);
        unreachable!();
    }
    let id = id.unwrap();
    unsafe {
        let data = &mut *state(component.get_nanbox_f64());
        data.admission += 1;
        data.origins.insert(
            id,
            Origin {
                slot,
                ids,
                reply: reply.clone(),
                admission: data.admission,
                ready: false,
                public_done: false,
                failed: false,
                error_reported: false,
                synchronous,
                retained: true,
                weak_promise: false,
            },
        );
    }
    poll::schedule(component.get_nanbox_f64());
    id
}

pub(super) fn call(
    component: f64,
    interface: &str,
    function: &str,
    args: f64,
    asynchronous: bool,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let args = scope.root_nanbox_f64(args);
    let retained = scope.root_nanbox_f64(undefined());
    let promise = scope.root_nanbox_f64(undefined());
    let saved = scope.root_nanbox_f64(undefined());
    let mut encoded = Value::Null;
    let mut request = Vec::new();
    let reply = ReplyKind::Call {
        interface: interface.to_owned(),
        function: function.to_owned(),
    };
    let outcome = crate::exception::catch_js_throw(|| {
        available(component.get_nanbox_f64(), !asynchronous);
        retained.set_nanbox_f64(value::retention::new_plan());
        let world = unsafe { &(*context::get(component.get_nanbox_f64())).world };
        let signature = context::signature(world, "exports", interface, function);
        let params = signature["params"]
            .as_array()
            .unwrap_or_else(|| fail("Invalid WIT parameter manifest"));
        encoded = value::typed::arguments(
            args.get_nanbox_f64(),
            params,
            world,
            component.get_nanbox_f64(),
            retained.get_nanbox_f64(),
        );
        request = serde_json::to_vec(&serde_json::json!({"op":"call", "interface":interface,"function":function,"args":encoded})).unwrap();
        let ids = if asynchronous {
            let captured = scope.root_raw_mut_ptr(crate::async_context::alloc_snapshot_holder(
                crate::async_context::capture_context(),
            ));
            promise.set_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::promise::js_promise_new() as i64,
            ));
            let pointer = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut();
            crate::promise::set_promise_context_snapshot(pointer, unsafe {
                (*captured.get_raw_mut_ptr::<crate::async_context::AsyncSnapshotHolder>())
                    .snapshot
                    .clone()
            });
            let (record, ids) = callback::saved::capture(promise.get_nanbox_f64());
            saved.set_nanbox_f64(record);
            ids
        } else {
            saved.set_nanbox_f64(callback::saved::current());
            AsyncResourceIds {
                async_id: 0,
                trigger_async_id: 0,
            }
        };
        let row = scope.root_nanbox_f64(records::create(
            promise.get_nanbox_f64(),
            retained.get_nanbox_f64(),
            saved.get_nanbox_f64(),
        ));
        let id = admit(
            component.get_nanbox_f64(),
            &request,
            row.get_nanbox_f64(),
            ids,
            &reply,
            !asynchronous,
        );
        if asynchronous {
            promise.get_nanbox_f64()
        } else {
            poll::drive(component.get_nanbox_f64(), id)
        }
    });
    drop((encoded, request, reply));
    finish(outcome)
}

/// Cleanup requests share admission and origins with ordinary calls. A failed
/// admission leaves the caller's identity untouched so finalization can retry.
pub(in crate::component) fn unit(component: f64, request: &Value) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let mut bytes = Vec::new();
    let outcome = crate::exception::catch_js_throw(|| {
        available(component.get_nanbox_f64(), true);
        bytes = serde_json::to_vec(request).unwrap();
        let saved = scope.root_nanbox_f64(callback::saved::current());
        let row = scope.root_nanbox_f64(records::create(
            undefined(),
            undefined(),
            saved.get_nanbox_f64(),
        ));
        let id = admit(
            component.get_nanbox_f64(),
            &bytes,
            row.get_nanbox_f64(),
            AsyncResourceIds {
                async_id: 0,
                trigger_async_id: 0,
            },
            &ReplyKind::Unit,
            true,
        );
        poll::drive(component.get_nanbox_f64(), id)
    });
    drop(bytes);
    finish(outcome)
}

pub(super) fn has_capacity(component: f64) -> bool {
    let data = unsafe { &*state(component) };
    data.origins.len() < MAX_CONTEXTS
        && data
            .origins
            .values()
            .filter(|meta| !meta.ready && !meta.public_done)
            .count()
            < perry_abi::component_host::MAX_PENDING_CALLS
}
pub(super) fn collection(component: f64, request: &Value) {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let mut bytes = Vec::new();
    let reply = ReplyKind::Collection(request.clone());
    let outcome = crate::exception::catch_js_throw(|| {
        available(component.get_nanbox_f64(), false);
        bytes = serde_json::to_vec(&if request["kind"] == "async-writer" {
            serde_json::json!({"op":"collect-writer","writer":request["writer"]})
        } else if request["kind"] == "writer-control" {
            request["request"].clone()
        } else if request["kind"] == "async-value" {
            serde_json::json!({"op":"collect","value":request["value"]})
        } else {
            serde_json::json!({"op":"resource-collect","request":request})
        })
        .unwrap();
        let saved = scope.root_nanbox_f64(callback::saved::current());
        let row = scope.root_nanbox_f64(records::create(
            undefined(),
            undefined(),
            saved.get_nanbox_f64(),
        ));
        admit(
            component.get_nanbox_f64(),
            &bytes,
            row.get_nanbox_f64(),
            AsyncResourceIds {
                async_id: 0,
                trigger_async_id: 0,
            },
            &reply,
            false,
        );
    });
    drop((bytes, reply));
    if let Err(error) = outcome {
        crate::exception::js_throw(error);
    }
}

pub(in crate::component) fn read(
    component: f64,
    target: f64,
    value: &Value,
    payload: &Value,
    stream: bool,
    capacity: u32,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let target = scope.root_nanbox_f64(target);
    let promise = scope.root_nanbox_f64(undefined());
    let mut request = Vec::new();
    let reply = ReplyKind::Read {
        payload: payload.clone(),
        stream,
    };
    let outcome = crate::exception::catch_js_throw(|| {
        available(component.get_nanbox_f64(), false);
        request =
            serde_json::to_vec(&serde_json::json!({"op":"read","value":value,"capacity":capacity}))
                .unwrap();
        let captured = scope.root_raw_mut_ptr(crate::async_context::alloc_snapshot_holder(
            crate::async_context::capture_context(),
        ));
        promise.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::promise::js_promise_new() as i64,
        ));
        let pointer = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
            .as_pointer::<crate::promise::Promise>()
            .cast_mut();
        crate::promise::set_promise_context_snapshot(pointer, unsafe {
            (*captured.get_raw_mut_ptr::<crate::async_context::AsyncSnapshotHolder>())
                .snapshot
                .clone()
        });
        let (saved, ids) = callback::saved::capture(promise.get_nanbox_f64());
        let saved = scope.root_nanbox_f64(saved);
        let weak = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(promise.get_nanbox_f64()) as i64,
        ));
        let row = scope.root_nanbox_f64(records::create(
            weak.get_nanbox_f64(),
            undefined(),
            saved.get_nanbox_f64(),
        ));
        let weak_target = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(target.get_nanbox_f64()) as i64,
        ));
        records::set(
            row.get_nanbox_f64(),
            records::TARGET,
            weak_target.get_nanbox_f64(),
        );
        crate::promise::operation_owner::register(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
            row.get_nanbox_f64(),
            records::PROMISE,
            target.get_nanbox_f64(),
        );
        if !super::super::async_value::pending_read_live(target.get_nanbox_f64()) {
            fail("Async value was closed during Promise initialization");
        }
        let id = admit(
            component.get_nanbox_f64(),
            &request,
            row.get_nanbox_f64(),
            ids,
            &reply,
            false,
        );
        unsafe {
            (*state(component.get_nanbox_f64()))
                .origins
                .get_mut(&id)
                .unwrap()
                .weak_promise = true;
        }
        cleanup::register(
            component.get_nanbox_f64(),
            promise.get_nanbox_f64(),
            serde_json::json!({"kind":"async-value","value":value}),
        );
        promise.get_nanbox_f64()
    });
    if outcome.is_err() && crate::promise::js_value_is_promise(promise.get_nanbox_f64()) != 0 {
        crate::promise::operation_owner::clear(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
        );
    }
    drop((request, reply));
    finish(outcome)
}
