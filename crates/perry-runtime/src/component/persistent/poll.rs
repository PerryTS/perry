//! One queued pin per active driver; all JS edges remain in the managed owner.
use super::*;
use crate::native_payload::OwnerLink;
use std::cell::{Cell, RefCell};
thread_local! {
    static DRIVERS: RefCell<Vec<OwnerLink>> = const { RefCell::new(Vec::new()) };
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}
unsafe extern "C" fn wake(agent: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    crate::event_pump::notify_agent_work(agent);
    #[cfg(target_arch = "wasm32")]
    let _ = agent;
}
fn retire(agent: crate::agent::AgentId) {
    if agent == crate::agent::current_agent() {
        DRIVERS.with(|queue| queue.borrow_mut().clear());
    }
}
pub(super) extern "C" fn has_active() -> i32 {
    i32::from(RUNNING.with(Cell::get) || DRIVERS.with(|queue| !queue.borrow().is_empty()))
}
pub(super) fn schedule(component: f64) {
    if unsafe { (*state(component)).queued || (*state(component)).retired } {
        return;
    }
    let link = payload::owner_link(owner(component), &PUMP).expect("live driver owner");
    unsafe {
        payload::link_ref(link);
        (*state(component)).queued = true;
    }
    crate::agent::register_retire_hook(retire);
    crate::stdlib_pump::js_register_agent_aux_pump(poll_components);
    crate::stdlib_pump::js_register_aux_has_active(has_active);
    DRIVERS.with(|queue| queue.borrow_mut().push(link));
    unsafe {
        wake(crate::agent::current_agent());
    }
}
fn pointer(promise: f64) -> *mut crate::promise::Promise {
    JSValue::from_bits(promise.to_bits())
        .as_pointer::<crate::promise::Promise>()
        .cast_mut()
}
fn pending(promise: f64) -> bool {
    unsafe { (*pointer(promise)).state == crate::promise::PromiseState::Pending }
}
fn first_error(component: f64, origin: u64) -> Option<f64> {
    let failed = unsafe {
        (*state(component))
            .origins
            .get(&origin)
            .is_some_and(|meta| meta.failed)
    };
    failed.then(|| records::get(origin_row(component, origin), records::ERROR))
}

/// Decode and settle one observer. Hook errors before settlement reject under
/// a cleanup scope; errors after settlement are surfaced without rewriting it.
fn complete(
    component: f64,
    origin: u64,
    reply: &Value,
    batch: &callbacks::Batch<'_>,
) -> Result<(), f64> {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let row = scope.root_nanbox_f64(origin_row(component.get_nanbox_f64(), origin));
    let promise = scope.root_nanbox_f64(records::get(row.get_nanbox_f64(), records::PROMISE));
    let result = scope.root_nanbox_f64(undefined());
    let frame = batch.frame(origin);
    let kind = unsafe {
        (*state(component.get_nanbox_f64()))
            .origins
            .get(&origin)
            .unwrap()
            .reply
            .clone()
    };
    let outcome = frame.run(|| {
        if let Some(error) = first_error(component.get_nanbox_f64(), origin) {
            crate::exception::js_throw(error);
        }
        if reply["ok"] != true {
            fail(
                reply["error"]
                    .as_str()
                    .unwrap_or("Component invocation failed"),
            );
        }
        if let ReplyKind::Call {
            interface,
            function,
        } = &kind
        {
            if reply["returned"] == true {
                let world = unsafe { &(*context::get(component.get_nanbox_f64())).world };
                let signature = context::signature(world, "exports", interface, function);
                result.set_nanbox_f64(value::decode(
                    &reply["value"],
                    &signature["result"],
                    world,
                    component.get_nanbox_f64(),
                ));
            }
        }
        if let ReplyKind::Collection(request) = &kind {
            host_resource::payload::collect_local(component.get_nanbox_f64(), request);
        }
        records::set(
            row.get_nanbox_f64(),
            records::RESULT,
            result.get_nanbox_f64(),
        );
        if !JSValue::from_bits(promise.get_nanbox_f64().to_bits()).is_undefined() {
            crate::promise::js_promise_resolve(
                pointer(promise.get_nanbox_f64()),
                result.get_nanbox_f64(),
            );
        }
        undefined()
    });
    let mut escaped = None;
    if let Err(error) = outcome {
        let error = scope.root_nanbox_f64(error);
        let public = !JSValue::from_bits(promise.get_nanbox_f64().to_bits()).is_undefined();
        if public && !pending(promise.get_nanbox_f64()) {
            escaped = Some(error.get_nanbox_f64());
        } else {
            remember(component.get_nanbox_f64(), origin, error.get_nanbox_f64());
            if public {
                if let Err(error) = frame.run_cleanup(|| {
                    crate::promise::js_promise_reject(
                        pointer(promise.get_nanbox_f64()),
                        records::get(row.get_nanbox_f64(), records::ERROR),
                    );
                    undefined()
                }) {
                    escaped = Some(error);
                }
            }
        }
    }
    // Rejections consume their original callback error. Background work can
    // record a new error only while the invocation remains retained.
    if outcome.is_err() {
        unsafe {
            (*state(component.get_nanbox_f64()))
                .origins
                .get_mut(&origin)
                .unwrap()
                .error_reported = true;
        }
    }
    // No JS/GC between obtaining an escaped value and returning it.
    unsafe {
        (*state(component.get_nanbox_f64()))
            .origins
            .get_mut(&origin)
            .unwrap()
            .public_done = true;
    }
    for column in [records::PROMISE, records::ARGUMENTS] {
        records::set(row.get_nanbox_f64(), column, undefined());
    }
    if !unsafe {
        (*state(component.get_nanbox_f64()))
            .origins
            .get(&origin)
            .unwrap()
            .synchronous
    } {
        records::set(row.get_nanbox_f64(), records::RESULT, undefined());
    }
    if matches!(kind, ReplyKind::Collection(_)) && outcome.is_err() {
        escaped = first_error(component.get_nanbox_f64(), origin).or(escaped);
    }
    drop(kind);
    escaped.map_or(Ok(()), Err)
}

fn discard_collector(component: f64, origin: u64) -> bool {
    if !unsafe {
        matches!(
            (*state(component)).origins.get(&origin).unwrap().reply,
            ReplyKind::Collection(_)
        )
    } {
        return false;
    }
    // Store retirement disposes remaining resources. This queue observer has
    // no public Promise and cancellation is not an application failure.
    let row = origin_row(component, origin);
    for column in [records::PROMISE, records::ARGUMENTS, records::RESULT] {
        records::set(row, column, undefined());
    }
    unsafe {
        (*state(component))
            .origins
            .get_mut(&origin)
            .unwrap()
            .public_done = true;
    }
    true
}

pub(super) fn reject_pending(component: f64, message: &str) {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let batch = callbacks::Batch::new(&scope, component.get_nanbox_f64(), true);
    let first = scope.root_nanbox_f64(undefined());
    let ids: Vec<_> = unsafe {
        (*state(component.get_nanbox_f64()))
            .origins
            .iter()
            .filter(|(_, meta)| !meta.ready && !meta.public_done)
            .map(|(id, _)| *id)
            .collect()
    };
    let reply = serde_json::json!({"ok":false,"error":message});
    let mut failed = false;
    for id in &ids {
        if discard_collector(component.get_nanbox_f64(), *id) {
            continue;
        }
        // complete uses normal hooks, then a cleanup fallback when they throw.
        if let Err(error) = complete(component.get_nanbox_f64(), *id, &reply, &batch) {
            if !failed {
                first.set_nanbox_f64(error);
                failed = true;
            }
        }
    }
    drop((ids, reply, batch));
    if failed {
        crate::exception::js_throw(first.get_nanbox_f64());
    }
}

fn prune(component: f64) {
    let gone: Vec<_> = unsafe {
        (*state(component))
            .origins
            .iter()
            .filter(|(_, meta)| meta.public_done && !meta.synchronous && !meta.retained)
            .map(|(id, meta)| (*id, meta.slot))
            .collect()
    };
    for (id, slot) in gone {
        records::release(records::table(owner(component)), slot);
        unsafe {
            let data = &mut *state(component);
            data.origins.remove(&id);
            data.free.push(slot);
        }
    }
}
fn needed(component: f64) -> bool {
    unsafe {
        let data = &*state(component);
        !data.retired
            && (data.active
                || !data.origins.is_empty()
                || !(*context::get(component)).pending_collections.is_empty())
    }
}

pub(super) fn cycle(component: f64) -> bool {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    if unsafe { (*state(component.get_nanbox_f64())).cycling } {
        return true;
    }
    unsafe {
        (*state(component.get_nanbox_f64())).cycling = true;
    }
    let primary = callbacks::Batch::new(&scope, component.get_nanbox_f64(), false);
    let cleanup = callbacks::Batch::new(&scope, component.get_nanbox_f64(), true);
    let first = scope.root_nanbox_f64(undefined());
    let mut failed = false;
    let mut delivered_failure = false;
    let background_before_poll = unsafe {
        (*state(component.get_nanbox_f64()))
            .origins
            .values()
            .any(|meta| meta.public_done)
    };
    let mut output = Vec::new();
    let mut report = Value::Null;
    let mut event = Value::Null;
    let outcome = crate::exception::catch_js_throw(|| {
        if !unsafe { (*state(component.get_nanbox_f64())).retired } {
            collections::drain_collections(component.get_nanbox_f64());
            imports::drain_replies(component.get_nanbox_f64(), &primary);
            if unsafe { (*state(component.get_nanbox_f64())).abort } {
                dispose(component.get_nanbox_f64());
            }
        }
        if !unsafe { (*state(component.get_nanbox_f64())).retired } {
            let mut buffer = Buffer::default();
            unsafe {
                (*state(component.get_nanbox_f64())).polling = true;
            }
            let status = unsafe {
                (host::ops().perry_component_driver_poll)(
                    (*context::get(component.get_nanbox_f64())).host,
                    primary.token(),
                    cleanup.token(),
                    Some(wake),
                    crate::agent::current_agent(),
                    &mut buffer,
                )
            };
            unsafe {
                (*state(component.get_nanbox_f64())).polling = false;
            }
            output = unsafe { take(buffer) };
            if status != OK {
                fail(&String::from_utf8_lossy(&output));
            }
            report = serde_json::from_slice(&output)
                .unwrap_or_else(|_| fail("Invalid persistent driver report"));
            let watermark = unsafe { (*state(component.get_nanbox_f64())).admission };
            let completed = report["completed"]
                .as_array_mut()
                .unwrap_or_else(|| fail("Invalid driver completion list"));
            // Stage the entire native batch before user hooks can dispose or
            // submit more calls. A ready reply owns its saved context until consumed.
            for reply in completed.drain(..) {
                let id = reply["id"]
                    .as_str()
                    .and_then(|id| id.parse::<u64>().ok())
                    .unwrap_or_else(|| fail("Invalid completion identity"));
                unsafe {
                    let data = &mut *state(component.get_nanbox_f64());
                    let meta = data
                        .origins
                        .get_mut(&id)
                        .unwrap_or_else(|| fail("Unknown completed invocation"));
                    meta.ready = true;
                    data.ready.insert(id, reply);
                    data.ready_order.push_back(id);
                }
            }
            let retained = report["retained"]
                .as_array()
                .unwrap_or_else(|| fail("Invalid retained origin list"));
            unsafe {
                let data = &mut *state(component.get_nanbox_f64());
                data.active = report["active"] == true;
                data.retired = report["retired"] == true;
                for (id, meta) in &mut data.origins {
                    if meta.admission <= watermark {
                        meta.retained = retained.iter().any(|item| {
                            item.as_str().and_then(|s| s.parse::<u64>().ok()) == Some(*id)
                        });
                    }
                }
            }
        }
        loop {
            let id = unsafe { (*state(component.get_nanbox_f64())).ready_order.pop_front() };
            let Some(id) = id else {
                break;
            };
            event = unsafe {
                (*state(component.get_nanbox_f64()))
                    .ready
                    .remove(&id)
                    .unwrap()
            };
            let observed = unsafe {
                (*state(component.get_nanbox_f64()))
                    .origins
                    .get(&id)
                    .unwrap()
                    .synchronous
            } || !JSValue::from_bits(
                records::get(origin_row(component.get_nanbox_f64(), id), records::PROMISE)
                    .to_bits(),
            )
            .is_undefined();
            let native_failure = event["ok"] != true;
            let cancelled = native_failure
                && unsafe { (*state(component.get_nanbox_f64())).retired }
                && first_error(component.get_nanbox_f64(), id).is_none();
            if cancelled && discard_collector(component.get_nanbox_f64(), id) {
                event = Value::Null;
                continue;
            }
            if let Err(error) = complete(component.get_nanbox_f64(), id, &event, &primary) {
                if !failed {
                    first.set_nanbox_f64(error);
                    failed = true;
                }
            }
            delivered_failure |= observed
                && (native_failure || first_error(component.get_nanbox_f64(), id).is_some());
            event = Value::Null;
        }
        if unsafe { (*state(component.get_nanbox_f64())).retired } {
            // Reject new submissions before running any disposal hooks.
            unsafe {
                (*context::get(component.get_nanbox_f64())).alive = false;
            }
            let rejected = crate::exception::catch_js_throw(|| {
                reject_pending(
                    component.get_nanbox_f64(),
                    report["error"]
                        .as_str()
                        .unwrap_or("Component has been disposed"),
                )
            });
            let disposed = cleanup.cleanup(|| {
                context::finish_disposal(component.get_nanbox_f64(), OK);
                undefined()
            });
            if !failed {
                if let Err(error) = rejected {
                    first.set_nanbox_f64(error);
                    failed = true;
                } else if !delivered_failure {
                    if let Err(error) = disposed {
                        first.set_nanbox_f64(error);
                        failed = true;
                    }
                }
            }
        }
        let background = unsafe {
            (*state(component.get_nanbox_f64()))
                .origins
                .iter()
                .find(|(_, meta)| meta.public_done && meta.failed && !meta.error_reported)
                .map(|(id, _)| *id)
        };
        if let Some(origin) = background {
            if !failed {
                first.set_nanbox_f64(first_error(component.get_nanbox_f64(), origin).unwrap());
                failed = true;
            }
            unsafe {
                (*state(component.get_nanbox_f64()))
                    .origins
                    .get_mut(&origin)
                    .unwrap()
                    .error_reported = true;
            }
        }
        if !failed
            && !delivered_failure
            && unsafe { (*state(component.get_nanbox_f64())).root_failed }
        {
            first.set_nanbox_f64(records::get(
                initial_row(owner(component.get_nanbox_f64())),
                records::ERROR,
            ));
            unsafe {
                (*state(component.get_nanbox_f64())).root_failed = false;
            }
            failed = true;
        }
        if !failed && (!delivered_failure || background_before_poll) && report["error"].is_string()
        {
            fail(report["error"].as_str().unwrap());
        }
        if delivered_failure {
            unsafe {
                (*state(component.get_nanbox_f64())).root_failed = false;
            }
        }
        undefined()
    });
    if let Err(error) = outcome {
        if !failed {
            first.set_nanbox_f64(error);
            failed = true;
        }
    }
    unsafe {
        (*state(component.get_nanbox_f64())).polling = false;
        (*state(component.get_nanbox_f64())).cycling = false;
    }
    prune(component.get_nanbox_f64());
    let active = needed(component.get_nanbox_f64());
    drop((primary, cleanup, output, report, event));
    if failed {
        crate::exception::js_throw(first.get_nanbox_f64());
    }
    active
}

pub(super) fn drive(component: f64, id: u64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let result = scope.root_nanbox_f64(undefined());
    let outcome = crate::exception::catch_js_throw(|| {
        loop {
            cycle(component.get_nanbox_f64());
            if unsafe {
                (*state(component.get_nanbox_f64()))
                    .origins
                    .get(&id)
                    .unwrap()
                    .public_done
            } {
                break;
            }
            std::thread::yield_now();
        }
        if let Some(error) = first_error(component.get_nanbox_f64(), id) {
            crate::exception::js_throw(error);
        }
        records::get(origin_row(component.get_nanbox_f64(), id), records::RESULT)
    });
    result.set_nanbox_f64(if outcome.is_err() {
        first_error(component.get_nanbox_f64(), id).unwrap_or_else(|| outcome.unwrap_err())
    } else {
        outcome.unwrap()
    });
    unsafe {
        (*state(component.get_nanbox_f64()))
            .origins
            .get_mut(&id)
            .unwrap()
            .synchronous = false;
    }
    prune(component.get_nanbox_f64());
    if outcome.is_err() {
        crate::exception::js_throw(result.get_nanbox_f64());
    }
    result.get_nanbox_f64()
}

pub(super) extern "C" fn poll_components() -> i32 {
    if RUNNING.with(|running| running.replace(true)) {
        return 0;
    }
    let scope = RuntimeHandleScope::new();
    let first = scope.root_nanbox_f64(undefined());
    let mut failed = false;
    let mut completed = 0i32;
    let links = DRIVERS.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    for link in links {
        let result = crate::exception::catch_js_throw(|| {
            unsafe { payload::link_event_owner(link) }.is_some_and(|driver_owner| {
                let component = payload::state_get(driver_owner, &PUMP, b"component");
                cycle(component)
            })
        });
        let active = match result {
            Ok(active) => active,
            Err(error) => {
                if !failed {
                    first.set_nanbox_f64(error);
                    failed = true;
                }
                unsafe { payload::link_event_owner(link) }.is_some_and(|driver_owner| {
                    needed(payload::state_get(driver_owner, &PUMP, b"component"))
                })
            }
        };
        if active {
            DRIVERS.with(|queue| queue.borrow_mut().push(link));
        } else {
            if let Some(driver_owner) = unsafe { payload::link_event_owner(link) } {
                let component = payload::state_get(driver_owner, &PUMP, b"component");
                unsafe {
                    (*state(component)).queued = false;
                }
            }
            unsafe {
                payload::link_unref(link);
            }
            completed = completed.saturating_add(1);
        }
    }
    RUNNING.with(|running| running.set(false));
    if failed {
        crate::exception::js_throw(first.get_nanbox_f64());
    }
    completed
}
