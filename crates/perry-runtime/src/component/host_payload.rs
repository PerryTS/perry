//! Payload storage is owned by the component object and scanned by ordinary GC.
use super::*;
use crate::object::owned_state_bag::{COMPONENT_HOST_NEXT_RESOURCE, COMPONENT_HOST_RESOURCES};

pub(super) const TYPE: u32 = 0;
pub(super) const PAYLOAD: u32 = 1;
const DESTRUCTOR: u32 = 2;
pub(super) const GENERATION: u32 = 3;
pub(super) const STATE: u32 = 4;
pub(super) const MATERIALIZED: u32 = 5;
pub(super) const LOCAL: f64 = 1.0;
const GUEST: f64 = 2.0;
const RETIRED: f64 = 3.0;

fn map_pointer(map: f64) -> *mut crate::map::MapHeader {
    crate::map::map_ptr_from_receiver_bits(map.to_bits())
        .unwrap_or_else(|| fail("Invalid host resource payload table"))
}
fn table(owner: f64, create: bool) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let map = unsafe {
        bag::load(
            object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_HOST_RESOURCES,
        )
    };
    if !JSValue::from_bits(map.to_bits()).is_undefined() || !create {
        return map;
    }
    let map = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::map::js_map_alloc(0) as i64
    ));
    unsafe {
        bag::store(
            object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_HOST_RESOURCES,
            map.get_nanbox_f64(),
        );
    }
    map.get_nanbox_f64()
}
pub(super) fn record(owner: f64, id: f64) -> f64 {
    let map = table(owner, false);
    if JSValue::from_bits(map.to_bits()).is_undefined() {
        fail("Host resource payload is not live");
    }
    let record = crate::map::js_map_get(map_pointer(map), id);
    if array_pointer(record).is_none() {
        fail("Host resource payload is not live");
    }
    record
}

#[no_mangle]
pub extern "C" fn js_component_host_resource_create(
    owner: f64,
    ty: f64,
    payload: f64,
    destructor: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let ty = scope.root_nanbox_f64(ty);
    let payload = scope.root_nanbox_f64(payload);
    let destructor = scope.root_nanbox_f64(destructor);
    let map = scope.root_nanbox_f64(undefined());
    let row = scope.root_nanbox_f64(undefined());
    let mut key = String::new();
    let mut id = 0;
    let result = crate::exception::catch_js_throw(|| {
        key = string(ty.get_nanbox_f64());
        let context = unsafe { &*context::get(owner.get_nanbox_f64()) };
        if !context.alive {
            fail("Component is disposed");
        }
        let provided = context.world["imports"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|interface| {
                interface["resources"]
                    .as_object()
                    .into_iter()
                    .flat_map(|map| map.values())
                    .any(|value| value.as_str() == Some(&key))
            });
        if !provided {
            fail("Resource type is not imported by this component");
        }
        if !JSValue::from_bits(destructor.get_nanbox_f64().to_bits()).is_undefined()
            && !crate::object::instanceof::value_is_callable(destructor.get_nanbox_f64())
        {
            fail("Host resource destructor must be callable");
        }
        map.set_nanbox_f64(table(owner.get_nanbox_f64(), true));
        let next = unsafe {
            bag::load(
                object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_HOST_NEXT_RESOURCE,
            )
        };
        let next = if JSValue::from_bits(next.to_bits()).is_undefined() {
            1.0
        } else {
            next
        };
        if next > u32::MAX as f64 {
            fail("Host resource identity space exhausted");
        }
        id = next as u32;
        unsafe {
            bag::store(
                object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_HOST_NEXT_RESOURCE,
                next + 1.0,
            );
        }
        row.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(6) as i64,
        ));
        for (index, value) in [
            (TYPE, ty.get_nanbox_f64()),
            (PAYLOAD, payload.get_nanbox_f64()),
            (DESTRUCTOR, destructor.get_nanbox_f64()),
            (GENERATION, 1.0),
            (STATE, LOCAL),
            (MATERIALIZED, 0.0),
        ] {
            set(row.get_nanbox_f64(), index, value);
        }
        crate::map::js_map_set(
            map_pointer(map.get_nanbox_f64()),
            id as f64,
            row.get_nanbox_f64(),
        );
        wrap(
            owner.get_nanbox_f64(),
            id,
            1,
            true,
            undefined(),
            undefined(),
            undefined(),
        )
    });
    if result.is_err() && id != 0 {
        crate::map::js_map_delete(map_pointer(map.get_nanbox_f64()), id as f64);
    }
    drop(key);
    finish(result)
}

fn reference(owner: f64, value: &Value, generation_delta: u32) -> (f64, f64) {
    let id = value["token"]
        .as_str()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|id| *id != 0)
        .unwrap_or_else(|| fail("Invalid host resource event identity")) as f64;
    let row = record(owner, id);
    let ty = string(get(row, TYPE));
    let matches = value["type"].as_str() == Some(ty.as_str())
        && value["generation"].as_u64()
            == Some(get(row, GENERATION) as u64 + generation_delta as u64);
    drop(ty);
    if !matches {
        fail("Host resource event has a stale or foreign identity");
    }
    (id, row)
}

pub(in crate::component) fn event(owner: f64, event: &Value) {
    match event["kind"].as_str() {
        Some("lower") => {
            let created = event["created"]
                .as_array()
                .unwrap_or_else(|| fail("Invalid host resource event"));
            let transferred = event["transferred"]
                .as_array()
                .unwrap_or_else(|| fail("Invalid host resource event"));
            // Validate every record before mutation. This path never invokes
            // application callbacks and cannot leave a partially consumed set.
            for value in created {
                let (_, row) = reference(owner, value, 0);
                if get(row, STATE) != LOCAL || get(row, MATERIALIZED) != 0.0 {
                    fail("Host resource was already registered or transferred");
                }
            }
            for value in transferred {
                let (_, row) = reference(owner, value, 0);
                if get(row, STATE) != LOCAL {
                    fail("Host resource was already transferred");
                }
            }
            for value in created {
                let (_, row) = reference(owner, value, 0);
                set(row, MATERIALIZED, 1.0);
            }
            for value in transferred {
                let (_, row) = reference(owner, value, 0);
                set(row, STATE, GUEST);
            }
        }
        Some("lift") => {
            let (_, row) = reference(owner, &event["resource"], 1);
            if get(row, STATE) != GUEST || get(row, MATERIALIZED) != 1.0 {
                fail("Host resource is already owned");
            }
            set(row, GENERATION, get(row, GENERATION) + 1.0);
            set(row, STATE, LOCAL);
        }
        Some("drop") => {
            let (id, _) = reference(owner, &event["resource"], 0);
            destroy(owner, id);
        }
        _ => fail("Unknown host resource event"),
    }
}

/// Release the JS payload now; the embedding retires its canonical handle
/// when the import callback returns. Keep only identity metadata until then.
pub(super) fn retire(owner: f64, id: f64) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let row = scope.root_nanbox_f64(record(owner.get_nanbox_f64(), id));
    let callback = scope.root_nanbox_f64(get(row.get_nanbox_f64(), DESTRUCTOR));
    let payload = scope.root_nanbox_f64(get(row.get_nanbox_f64(), PAYLOAD));
    let reference = serde_json::json!({"token":(id as u32).to_string(),
        "type":string(get(row.get_nanbox_f64(), TYPE)),
        "generation":get(row.get_nanbox_f64(), GENERATION) as u32});
    unsafe {
        (*context::get(owner.get_nanbox_f64()))
            .pending_resource_drops
            .push(reference);
    }
    set(row.get_nanbox_f64(), STATE, RETIRED);
    set(row.get_nanbox_f64(), PAYLOAD, undefined());
    set(row.get_nanbox_f64(), DESTRUCTOR, undefined());
    invoke_destructor(callback.get_nanbox_f64(), payload.get_nanbox_f64());
}

pub(super) fn destroy(owner: f64, id: f64) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let row = scope.root_nanbox_f64(record(owner.get_nanbox_f64(), id));
    let map = scope.root_nanbox_f64(table(owner.get_nanbox_f64(), false));
    crate::map::js_map_delete(map_pointer(map.get_nanbox_f64()), id);
    let callback = scope.root_nanbox_f64(get(row.get_nanbox_f64(), DESTRUCTOR));
    let payload = scope.root_nanbox_f64(get(row.get_nanbox_f64(), PAYLOAD));
    invoke_destructor(callback.get_nanbox_f64(), payload.get_nanbox_f64());
}

/// Native canonical handles are collected by the engine. A host-created
/// resource not yet passed to it has only a local payload to release.
pub(in crate::component) fn collect_local(owner: f64, request: &Value) {
    if request["kind"] != "host" || !unsafe { (*context::get(owner)).alive } {
        return;
    }
    let identity = &request["resource"];
    let Some(id) = identity["token"]
        .as_str()
        .and_then(|token| token.parse::<u32>().ok())
    else {
        return;
    };
    let map = table(owner, false);
    if JSValue::from_bits(map.to_bits()).is_undefined() {
        return;
    }
    let row = crate::map::js_map_get(map_pointer(map), id as f64);
    if array_pointer(row).is_none()
        || get(row, STATE) != LOCAL
        || get(row, MATERIALIZED) != 0.0
        || identity["generation"].as_u64() != Some(get(row, GENERATION) as u64)
        || identity["type"].as_str() != Some(string(get(row, TYPE)).as_str())
    {
        return;
    }
    destroy(owner, id as f64);
}

fn invoke_destructor(callback: f64, payload: f64) {
    let scope = RuntimeHandleScope::new();
    let callback = scope.root_nanbox_f64(callback);
    let payload = scope.root_nanbox_f64(payload);
    if !JSValue::from_bits(callback.get_nanbox_f64().to_bits()).is_undefined() {
        let args = [payload.get_nanbox_f64()];
        unsafe {
            crate::closure::js_native_call_value(
                callback.get_nanbox_f64(),
                crate::closure::plain_call_receiver(),
                args.as_ptr(),
                1,
            );
        }
    }
}

pub(in crate::component) fn dispose_owner(owner: f64) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    unsafe {
        (*context::get(owner.get_nanbox_f64())).alive = false;
        (*context::get(owner.get_nanbox_f64()))
            .pending_resource_drops
            .clear();
        (*context::get(owner.get_nanbox_f64()))
            .pending_collections
            .clear();
    }
    let map = scope.root_nanbox_f64(table(owner.get_nanbox_f64(), false));
    if JSValue::from_bits(map.get_nanbox_f64().to_bits()).is_undefined() {
        return;
    }
    let keys = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(crate::map::js_map_keys(
        map_pointer(map.get_nanbox_f64()),
    ) as i64));
    let error = scope.root_nanbox_f64(undefined());
    let mut failed = false;
    for index in 0..crate::array::js_array_length(array(keys.get_nanbox_f64())) {
        let id = get(keys.get_nanbox_f64(), index);
        let row = crate::map::js_map_get(map_pointer(map.get_nanbox_f64()), id);
        if array_pointer(row).is_none() {
            continue;
        } // an earlier destructor disposed it
        if let Err(thrown) =
            crate::exception::catch_js_throw(|| destroy(owner.get_nanbox_f64(), id))
        {
            if !failed {
                error.set_nanbox_f64(thrown);
                failed = true;
            }
        }
    }
    if failed {
        crate::exception::js_throw(error.get_nanbox_f64());
    }
}
