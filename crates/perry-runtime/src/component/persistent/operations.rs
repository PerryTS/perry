//! Producer commands retain typed argument plans until ownership receipts.
//! Pending writes use the same observation custody as readable operations.
use super::*;

pub(in crate::component) fn cancel(component: f64, writer: &Value) {
    let request = serde_json::json!({"kind":"writer-control","request":{"op":"cancel-write","writer":writer}});
    let outcome = crate::exception::catch_js_throw(|| submit::collection(component, &request));
    drop(request);
    if let Err(error) = outcome {
        crate::exception::js_throw(error);
    }
}

pub(in crate::component) fn create(component: f64, index: u64, schema: &Value) -> f64 {
    let request = serde_json::json!({"op":"create","index":index});
    let reply = ReplyKind::Create {
        schema: schema.clone(),
    };
    let outcome = crate::exception::catch_js_throw(|| {
        admit_producer(component, undefined(), undefined(), &request, &reply, None)
    });
    drop((request, reply));
    finish(outcome)
}

pub(in crate::component) fn write(
    component: f64,
    target: f64,
    values: &Value,
    retained: f64,
    payload: &Value,
    stream: bool,
    writer: &Value,
) -> f64 {
    let request = serde_json::json!({"op":"write","writer":writer,"values":values});
    let reply = ReplyKind::Write {
        payload: payload.clone(),
        stream,
    };
    let collection = serde_json::json!({"kind":"async-writer","writer":writer});
    let outcome = crate::exception::catch_js_throw(|| {
        admit_producer(
            component,
            target,
            retained,
            &request,
            &reply,
            Some(&collection),
        )
    });
    drop((request, reply, collection));
    finish(outcome)
}

fn admit_producer(
    component: f64,
    target: f64,
    retained: f64,
    request: &Value,
    reply: &ReplyKind,
    collection: Option<&Value>,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let target = scope.root_nanbox_f64(target);
    let retained = scope.root_nanbox_f64(retained);
    let promise = scope.root_nanbox_f64(undefined());
    let weak = collection.is_some();
    let mut bytes = Vec::new();
    let outcome = crate::exception::catch_js_throw(|| {
        submit::available(component.get_nanbox_f64(), false);
        bytes = serde_json::to_vec(request).unwrap();
        let captured = scope.root_raw_mut_ptr(crate::async_context::alloc_snapshot_holder(
            crate::async_context::capture_context(),
        ));
        promise.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::promise::js_promise_new() as i64,
        ));
        let pointer = || {
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut()
        };
        crate::promise::set_promise_context_snapshot(pointer(), unsafe {
            (*captured.get_raw_mut_ptr::<crate::async_context::AsyncSnapshotHolder>())
                .snapshot
                .clone()
        });
        let (saved, ids) = callback::saved::capture(promise.get_nanbox_f64());
        let saved = scope.root_nanbox_f64(saved);
        let observer = scope.root_nanbox_f64(if weak {
            crate::value::js_nanbox_pointer(
                crate::weakref::js_weakref_new(promise.get_nanbox_f64()) as i64,
            )
        } else {
            promise.get_nanbox_f64()
        });
        let row = scope.root_nanbox_f64(records::create(
            observer.get_nanbox_f64(),
            retained.get_nanbox_f64(),
            saved.get_nanbox_f64(),
        ));
        if weak {
            let target_ref = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::weakref::js_weakref_new(target.get_nanbox_f64()) as i64,
            ));
            records::set(
                row.get_nanbox_f64(),
                records::TARGET,
                target_ref.get_nanbox_f64(),
            );
            crate::promise::operation_owner::register(
                pointer(),
                row.get_nanbox_f64(),
                records::PROMISE,
                target.get_nanbox_f64(),
            );
            if !super::super::writer::pending_live(target.get_nanbox_f64()) {
                fail("Async writer was closed during Promise initialization");
            }
        }
        let id = submit::admit(
            component.get_nanbox_f64(),
            &bytes,
            row.get_nanbox_f64(),
            ids,
            reply,
            false,
        );
        unsafe {
            (*state(component.get_nanbox_f64()))
                .origins
                .get_mut(&id)
                .unwrap()
                .weak_promise = weak;
        }
        if let Some(collection) = collection {
            cleanup::register(
                component.get_nanbox_f64(),
                promise.get_nanbox_f64(),
                collection.clone(),
            );
        }
        promise.get_nanbox_f64()
    });
    if outcome.is_err() && crate::promise::js_value_is_promise(promise.get_nanbox_f64()) != 0 {
        crate::promise::operation_owner::clear(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
        );
    }
    drop(bytes);
    finish(outcome)
}
