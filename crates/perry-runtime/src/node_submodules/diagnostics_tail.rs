//! Node 26 diagnostics_channel tail APIs: BoundedChannel and store scopes.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::array::{js_array_get_f64, js_array_length};
use crate::closure::{
    js_closure_alloc, js_closure_call1, js_closure_get_capture_ptr, js_closure_set_capture_ptr,
    js_register_closure_arity, js_register_closure_synthetic_arguments, ClosureHeader,
};
use crate::object::{js_object_alloc, ObjectHeader};
use crate::value::{js_nanbox_get_pointer, JSValue};

use super::diagnostics::*;

struct DiagBoundedState {
    obj: *mut ObjectHeader,
    events: [i64; 2],
}

struct DiagStoreScopeState {
    handles: Vec<i64>,
    disposed: bool,
}

thread_local! {
    static DIAG_BOUNDED_CHANNELS: RefCell<HashMap<i64, DiagBoundedState>> = RefCell::new(HashMap::new());
    static DIAG_STORE_SCOPES: RefCell<HashMap<i64, DiagStoreScopeState>> = RefCell::new(HashMap::new());
}

pub(crate) fn scan_diagnostics_tail_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    DIAG_BOUNDED_CHANNELS.with(|m| {
        for state in m.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut state.obj);
        }
    });
}

pub(crate) fn diagnostics_bounded_channel_is_instance_value(value: f64) -> bool {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return false;
    }
    let ptr = js_value.as_pointer::<ObjectHeader>();
    DIAG_BOUNDED_CHANNELS.with(|channels| {
        channels
            .borrow()
            .values()
            .any(|channel| std::ptr::eq(channel.obj, ptr))
    })
}

fn unbox_arg_array(arr_value: f64) -> Vec<f64> {
    let arr = js_nanbox_get_pointer(arr_value) as *const crate::array::ArrayHeader;
    if arr.is_null() {
        return Vec::new();
    }
    let len = js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push(js_array_get_f64(arr, i));
    }
    out
}

fn is_symbol_value(value: f64) -> bool {
    let bits = value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    tag == crate::value::POINTER_TAG && unsafe { crate::symbol::js_is_symbol(value) } != 0
}

fn store_handle(store: f64) -> Option<i64> {
    let bits = store.to_bits();
    if bits & crate::value::TAG_MASK == crate::value::INT32_TAG {
        return Some((bits & crate::value::INT32_MASK) as i32 as i64);
    }
    if bits & crate::value::TAG_MASK == crate::value::POINTER_TAG {
        let raw = (bits & crate::value::POINTER_MASK) as i64;
        if raw > 0 && raw < 0x10000 {
            return Some(raw);
        }
    }
    if store.is_finite() {
        return Some(store as i64);
    }
    None
}

fn install_dispose(obj: *mut ObjectHeader, method: f64) {
    set_field_value(obj, "__perry_dispose__", method);
    set_field_value(obj, "@@__perry_wk_dispose", method);
    let dispose = crate::symbol::well_known_symbol("dispose");
    if !dispose.is_null() {
        let obj_value = boxed_ptr(obj);
        let symbol_value = f64::from_bits(JSValue::pointer(dispose as *const u8).bits());
        unsafe {
            crate::symbol::js_object_set_symbol_property(obj_value, symbol_value, method);
        }
    }
}

pub(crate) extern "C" fn diag_store_scope_dispose(closure: *const ClosureHeader) -> f64 {
    let scope_id = js_closure_get_capture_ptr(closure, 0);
    let handles = DIAG_STORE_SCOPES.with(|m| {
        let mut m = m.borrow_mut();
        let Some(scope) = m.get_mut(&scope_id) else {
            return Vec::new();
        };
        if scope.disposed {
            return Vec::new();
        }
        scope.disposed = true;
        scope.handles.clone()
    });
    for handle in handles.into_iter().rev() {
        crate::async_context::pop_store(handle);
    }
    undefined()
}

// Not wired to a Channel method: Node has no `withStoreScope` on the
// diagnostics_channel Channel. Retained (unused) so the store-scope machinery
// it exercises stays available if a future Node revision adds the API.
#[allow(dead_code)]
pub(crate) extern "C" fn diag_channel_with_store_scope(
    closure: *const ClosureHeader,
    data: f64,
) -> f64 {
    let id = method_id(closure);
    let stores = DIAG_CHANNELS.with(|m| {
        m.borrow()
            .get(&id)
            .map(|c| c.stores.clone())
            .unwrap_or_default()
    });
    let mut handles = Vec::new();
    for (store, transform) in stores {
        let context = match transform {
            StoreTransform::Callable(t) => {
                match catch_js(|| js_closure_call1(closure_ptr(t), data)) {
                    Ok(context) => context,
                    Err(err) => {
                        schedule_uncaught(err);
                        continue;
                    }
                }
            }
            StoreTransform::NonCallable => {
                schedule_uncaught(make_transform_not_a_function_error());
                continue;
            }
            StoreTransform::None => data,
        };
        if let Some(handle) = store_handle(store) {
            crate::async_context::push_store(handle, context);
            handles.push(handle);
        }
    }

    let scope_id = next_diag_id();
    DIAG_STORE_SCOPES.with(|m| {
        m.borrow_mut().insert(
            scope_id,
            DiagStoreScopeState {
                handles,
                disposed: false,
            },
        );
    });
    let obj = js_object_alloc(0, 3);
    let dispose = js_closure_alloc(cast0(diag_store_scope_dispose), 1);
    js_closure_set_capture_ptr(dispose, 0, scope_id);
    js_register_closure_arity(cast0(diag_store_scope_dispose), 0);
    let dispose_value = boxed_ptr(dispose);
    install_dispose(obj, dispose_value);
    boxed_ptr(obj)
}

fn bounded_events(id: i64) -> [i64; 2] {
    DIAG_BOUNDED_CHANNELS.with(|m| {
        m.borrow()
            .get(&id)
            .map(|bounded| bounded.events)
            .unwrap_or([0; 2])
    })
}

fn bounded_active(events: [i64; 2]) -> bool {
    DIAG_CHANNELS.with(|channels| {
        let channels = channels.borrow();
        events.iter().copied().any(|id| {
            channels.get(&id).is_some_and(|channel| {
                get_field_value(channel.obj, "hasSubscribers").to_bits() == TAG_TRUE
                    || !channel.subscribers.is_empty()
                    || !channel.stores.is_empty()
            })
        })
    })
}

pub(crate) fn update_all_bounded_active() {
    DIAG_BOUNDED_CHANNELS.with(|bounded| {
        for state in bounded.borrow_mut().values_mut() {
            set_field_value(
                state.obj,
                "hasSubscribers",
                bool_value(bounded_active(state.events)),
            );
        }
    });
}

pub(crate) extern "C" fn diag_bounded_subscribe(
    closure: *const ClosureHeader,
    handlers: f64,
) -> f64 {
    let events = bounded_events(method_id(closure));
    for (idx, name) in ["start", "end"].iter().enumerate() {
        let h = get_field_value(js_nanbox_get_pointer(handlers) as *mut ObjectHeader, name);
        let h_bits = h.to_bits();
        if h_bits == TAG_UNDEFINED || h_bits == crate::value::TAG_NULL {
            continue;
        }
        if !valid_closure_value(h) {
            throw_invalid_arg();
        }
        add_subscriber(events[idx], h);
    }
    undefined()
}

pub(crate) extern "C" fn diag_bounded_unsubscribe(
    closure: *const ClosureHeader,
    handlers: f64,
) -> f64 {
    let events = bounded_events(method_id(closure));
    let mut ok = true;
    for (idx, name) in ["start", "end"].iter().enumerate() {
        let h = get_field_value(js_nanbox_get_pointer(handlers) as *mut ObjectHeader, name);
        if valid_closure_value(h) && !remove_subscriber(events[idx], h) {
            ok = false;
        }
    }
    bool_value(ok)
}

pub(crate) extern "C" fn diag_bounded_run(closure: *const ClosureHeader, all_args: f64) -> f64 {
    let all = unbox_arg_array(all_args);
    let undef = undefined();
    let context = all.first().copied().unwrap_or(undef);
    let fn_value = all.get(1).copied().unwrap_or(undef);
    let this_arg = all.get(2).copied().unwrap_or(undef);
    let args: Vec<f64> = if all.len() > 3 {
        all[3..].to_vec()
    } else {
        Vec::new()
    };
    if !valid_closure_value(fn_value) {
        crate::closure::throw_not_callable();
    }
    let events = bounded_events(method_id(closure));
    publish_channel(events[0], context);
    match catch_js(|| call_fn_value(fn_value, this_arg, &args)) {
        Ok(result) => {
            publish_channel(events[1], context);
            result
        }
        Err(err) => {
            publish_channel(events[1], context);
            crate::exception::js_throw(err)
        }
    }
}

pub(crate) extern "C" fn diag_bounded_scope_dispose(closure: *const ClosureHeader) -> f64 {
    if js_closure_get_capture_ptr(closure, 2) != 0 {
        return undefined();
    }
    let end = js_closure_get_capture_ptr(closure, 0);
    let context = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    js_closure_set_capture_ptr(closure as *mut ClosureHeader, 2, 1);
    publish_channel(end, context);
    undefined()
}

pub(crate) extern "C" fn diag_bounded_with_scope(
    closure: *const ClosureHeader,
    context: f64,
) -> f64 {
    let events = bounded_events(method_id(closure));
    publish_channel(events[0], context);
    let obj = js_object_alloc(0, 3);
    let dispose = js_closure_alloc(cast0(diag_bounded_scope_dispose), 3);
    js_closure_set_capture_ptr(dispose, 0, events[1]);
    js_closure_set_capture_ptr(dispose, 1, context.to_bits() as i64);
    js_closure_set_capture_ptr(dispose, 2, 0);
    js_register_closure_arity(cast0(diag_bounded_scope_dispose), 0);
    let dispose_value = boxed_ptr(dispose);
    install_dispose(obj, dispose_value);
    boxed_ptr(obj)
}

fn bounded_run_method_closure(id: i64) -> f64 {
    let func = cast1(diag_bounded_run);
    let c = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(c, 0, id);
    js_register_closure_synthetic_arguments(func, 0);
    boxed_ptr(c)
}

pub(crate) extern "C" fn thunk_diag_bounded_channel(
    _closure: *const ClosureHeader,
    name_or_channels: f64,
) -> f64 {
    if is_symbol_value(name_or_channels) {
        throw_invalid_arg();
    }
    let events = if decode_string_value(name_or_channels).is_some() {
        [
            ensure_channel(tracing_event_name(name_or_channels, "start")),
            ensure_channel(tracing_event_name(name_or_channels, "end")),
        ]
    } else if (name_or_channels.to_bits() & crate::value::TAG_MASK) == crate::value::POINTER_TAG {
        [
            channel_from_object_property(name_or_channels, "start"),
            channel_from_object_property(name_or_channels, "end"),
        ]
    } else {
        throw_invalid_arg();
    };
    let id = next_diag_id();
    let obj = js_object_alloc(0, 7);
    set_field_value(obj, "start", channel_obj(events[0]));
    set_field_value(obj, "end", channel_obj(events[1]));
    set_field_value(obj, "hasSubscribers", bool_value(bounded_active(events)));
    set_field_value(
        obj,
        "subscribe",
        method_closure(cast1(diag_bounded_subscribe), 1, id),
    );
    set_field_value(
        obj,
        "unsubscribe",
        method_closure(cast1(diag_bounded_unsubscribe), 1, id),
    );
    set_field_value(obj, "run", bounded_run_method_closure(id));
    set_field_value(
        obj,
        "withScope",
        method_closure(cast1(diag_bounded_with_scope), 1, id),
    );
    DIAG_BOUNDED_CHANNELS.with(|m| {
        m.borrow_mut().insert(id, DiagBoundedState { obj, events });
    });
    boxed_ptr(obj)
}
