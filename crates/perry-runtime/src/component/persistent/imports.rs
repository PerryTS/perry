//! Pending import Promises and committed response roots belong to managed rows.
use super::*;
use crate::promise::{Promise, PromiseState};
const REQUEST: u32 = 0;
const LOW: u32 = 1;
const HIGH: u32 = 2;
const PROMISE: u32 = 3;
const LIFETIME: u32 = 4;
const TYPE: u32 = 5;
fn import_list(driver_owner: f64) -> f64 {
    payload::state_get(driver_owner, &PUMP, b"imports")
}
fn text(bytes: &[u8]) -> f64 {
    crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ) as i64)
}
fn schema_string(ty: &Value) -> f64 {
    let bytes = serde_json::to_vec(ty).expect("native WIT schema");
    let outcome = crate::exception::catch_js_throw(|| text(&bytes));
    drop(bytes);
    finish(outcome)
}
fn expire(record: f64) {
    let lifetime = records::get(record, LIFETIME);
    if let Some(pointer) = array_pointer(lifetime) {
        crate::array::js_array_set_f64(pointer, 0, 0.0);
    }
}
pub(in crate::component) fn register(
    component: f64,
    origin: u64,
    request: &str,
    value: f64,
    lifetime: f64,
    ty: &Value,
) {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let value = scope.root_nanbox_f64(value);
    let lifetime = scope.root_nanbox_f64(lifetime);
    let schema = scope.root_nanbox_f64(schema_string(ty));
    let request = scope.root_nanbox_f64(text(request.as_bytes()));
    let promise =
        scope.root_raw_mut_ptr(crate::promise::js_promise_resolved(value.get_nanbox_f64()));
    crate::promise::js_promise_mark_internally_handled(promise.get_raw_mut_ptr());
    let record = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(6));
    for (index, value) in [
        request.get_nanbox_f64(),
        (origin as u32) as f64,
        ((origin >> 32) as u32) as f64,
        crate::value::js_nanbox_pointer(promise.get_raw_mut_ptr::<Promise>() as i64),
        lifetime.get_nanbox_f64(),
        schema.get_nanbox_f64(),
    ]
    .into_iter()
    .enumerate()
    {
        crate::array::js_array_set_f64(record.get_raw_mut_ptr(), index as u32, value);
    }
    let driver_owner = scope.root_nanbox_f64(owner(component.get_nanbox_f64()));
    let entries = scope.root_nanbox_f64(import_list(driver_owner.get_nanbox_f64()));
    let count = crate::array::js_array_length(array_pointer(entries.get_nanbox_f64()).unwrap());
    for index in 0..count {
        if array_pointer(records::row(entries.get_nanbox_f64(), index)).is_none() {
            crate::array::js_array_set_f64(
                array_pointer(entries.get_nanbox_f64()).unwrap(),
                index,
                crate::value::js_nanbox_pointer(
                    record.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
                ),
            );
            return;
        }
    }
    let grown = crate::array::js_array_set_f64_extend(
        array_pointer(entries.get_nanbox_f64()).unwrap(),
        count,
        crate::value::js_nanbox_pointer(
            record.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64
        ),
    );
    payload::state_set(
        driver_owner.get_nanbox_f64(),
        &PUMP,
        b"imports",
        crate::value::js_nanbox_pointer(grown as i64),
    );
}
pub(in crate::component) fn new_response_plan(component: f64, origin: u64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let plan = scope.root_nanbox_f64(value::retention::new_plan());
    let row = scope.root_nanbox_f64(origin_row(component.get_nanbox_f64(), origin));
    let plans = scope.root_nanbox_f64(records::get(row.get_nanbox_f64(), records::IMPORT_RESULTS));
    if array_pointer(plans.get_nanbox_f64()).is_none() {
        plans.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc(0) as i64,
        ));
        records::set(
            row.get_nanbox_f64(),
            records::IMPORT_RESULTS,
            plans.get_nanbox_f64(),
        );
    }
    let count = crate::array::js_array_length(array_pointer(plans.get_nanbox_f64()).unwrap());
    for index in 0..count {
        if array_pointer(records::row(plans.get_nanbox_f64(), index)).is_none() {
            crate::array::js_array_set_f64(
                array_pointer(plans.get_nanbox_f64()).unwrap(),
                index,
                plan.get_nanbox_f64(),
            );
            return plan.get_nanbox_f64();
        }
    }
    let grown = crate::array::js_array_set_f64_extend(
        array_pointer(plans.get_nanbox_f64()).unwrap(),
        count,
        plan.get_nanbox_f64(),
    );
    records::set(
        row.get_nanbox_f64(),
        records::IMPORT_RESULTS,
        crate::value::js_nanbox_pointer(grown as i64),
    );
    plan.get_nanbox_f64()
}
pub(in crate::component) fn prepare_commits(
    component: f64,
    origin: u64,
    event: &Value,
) -> Vec<(u32, Vec<u32>)> {
    if event["kind"] != "lower" {
        return Vec::new();
    }
    let row = origin_row(component, origin);
    let mut prepared = Vec::new();
    let arguments = records::get(row, records::ARGUMENTS);
    if array_pointer(arguments).is_some() {
        prepared.push((u32::MAX, value::retention::transferred(arguments, event)));
    }
    let plans = records::get(row, records::IMPORT_RESULTS);
    if let Some(pointer) = array_pointer(plans) {
        for index in 0..crate::array::js_array_length(pointer) {
            let plan = records::row(plans, index);
            if array_pointer(plan).is_some() {
                prepared.push((index, value::retention::transferred(plan, event)));
            }
        }
    }
    prepared
}
pub(in crate::component) fn release_commits(
    component: f64,
    origin: u64,
    prepared: &[(u32, Vec<u32>)],
) {
    let row = origin_row(component, origin);
    let plans = records::get(row, records::IMPORT_RESULTS);
    for (index, slots) in prepared {
        let plan = if *index == u32::MAX {
            records::get(row, records::ARGUMENTS)
        } else {
            records::row(plans, *index)
        };
        for slot in slots {
            value::retention::release(plan, *slot);
        }
        if value::retention::is_empty(plan) {
            if *index == u32::MAX {
                records::set(row, records::ARGUMENTS, undefined());
            } else {
                records::release(plans, *index);
            }
        }
    }
}

pub(super) fn drain_replies(component: f64, batch: &callbacks::Batch<'_>) {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let entries = scope.root_nanbox_f64(import_list(owner(component.get_nanbox_f64())));
    let Some(pointer) = array_pointer(entries.get_nanbox_f64()) else {
        return;
    };
    let count = crate::array::js_array_length(pointer);
    for index in 0..count {
        if !unsafe { (*context::get(component.get_nanbox_f64())).alive } {
            break;
        }
        let record = scope.root_nanbox_f64(records::row(entries.get_nanbox_f64(), index));
        if array_pointer(record.get_nanbox_f64()).is_none() {
            continue;
        }
        let promise = scope.root_nanbox_f64(records::get(record.get_nanbox_f64(), PROMISE));
        let pointer =
            JSValue::from_bits(promise.get_nanbox_f64().to_bits()).as_pointer::<Promise>();
        let progress = unsafe { (*pointer).state };
        if progress == PromiseState::Pending {
            continue;
        }
        let origin = (records::get(record.get_nanbox_f64(), LOW) as u64)
            | ((records::get(record.get_nanbox_f64(), HIGH) as u64) << 32);
        let value = scope.root_nanbox_f64(unsafe {
            if progress == PromiseState::Fulfilled {
                (*pointer).value
            } else {
                (*pointer).reason
            }
        });
        let mut ty = Value::Null;
        let mut encoded = Vec::new();
        let mut output = Vec::new();
        let outcome = batch.frame(origin).run(|| {
            if !unsafe { (*context::get(component.get_nanbox_f64())).alive } {
                fail("Component has been disposed");
            }
            let request = string(records::get(record.get_nanbox_f64(), REQUEST))
                .parse::<u64>()
                .unwrap_or_else(|_| fail("Invalid native import request"));
            encoded = if progress == PromiseState::Fulfilled {
                ty = serde_json::from_str(&string(records::get(record.get_nanbox_f64(), TYPE)))
                    .unwrap_or_else(|_| fail("Invalid saved result schema"));
                let plan =
                    scope.root_nanbox_f64(new_response_plan(component.get_nanbox_f64(), origin));
                let world = unsafe { &(*context::get(component.get_nanbox_f64())).world };
                serde_json::to_vec(&value::typed::result(
                    value.get_nanbox_f64(),
                    &ty,
                    world,
                    component.get_nanbox_f64(),
                    plan.get_nanbox_f64(),
                ))
                .unwrap()
            } else {
                remember(component.get_nanbox_f64(), origin, value.get_nanbox_f64());
                b"JavaScript async import rejected".to_vec()
            };
            let mut buffer = Buffer::default();
            let status = unsafe {
                (host::ops().perry_component_host_async_import_resolve)(
                    (*context::get(component.get_nanbox_f64())).host,
                    request,
                    progress == PromiseState::Fulfilled,
                    encoded.as_ptr(),
                    encoded.len(),
                    &mut buffer,
                )
            };
            output = unsafe { take(buffer) };
            if status != OK {
                fail(&String::from_utf8_lossy(&output));
            }
            undefined()
        });
        expire(record.get_nanbox_f64());
        records::release(entries.get_nanbox_f64(), index);
        drop((ty, encoded, output));
        if let Err(error) = outcome {
            remember(component.get_nanbox_f64(), origin, error);
            unsafe {
                (*state(component.get_nanbox_f64())).abort = true;
            }
        }
    }
}
pub(in crate::component) fn clear(component: f64) {
    let scope = RuntimeHandleScope::new();
    let driver_owner = scope.root_nanbox_f64(owner(component));
    let entries = scope.root_nanbox_f64(import_list(driver_owner.get_nanbox_f64()));
    if let Some(pointer) = array_pointer(entries.get_nanbox_f64()) {
        for index in 0..crate::array::js_array_length(pointer) {
            let record = records::row(entries.get_nanbox_f64(), index);
            if array_pointer(record).is_some() {
                expire(record);
            }
        }
    }
    payload::state_set(
        driver_owner.get_nanbox_f64(),
        &PUMP,
        b"imports",
        undefined(),
    );
}
