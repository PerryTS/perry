//! Snapshot wrapper identities during encoding. A fixed holder owns the growing
//! pair list, so list growth never leaves a stale header in the operation.
use super::*;
use crate::value::js_nanbox_pointer;

pub(in crate::component) fn new_plan() -> f64 {
    let scope = RuntimeHandleScope::new();
    let list = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    let plan = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(1));
    crate::array::js_array_set_f64(
        plan.get_raw_mut_ptr(),
        0,
        js_nanbox_pointer(list.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64),
    );
    js_nanbox_pointer(plan.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64)
}

fn list(plan: f64) -> f64 {
    crate::array::js_array_get_f64(
        super::super::array_pointer(plan).expect("private retention plan"),
        0,
    )
}

pub(super) fn retain(plan: f64, wrapper: f64, identity: &Value) -> Result<(), f64> {
    let scope = RuntimeHandleScope::new();
    let plan = scope.root_nanbox_f64(plan);
    let wrapper = scope.root_nanbox_f64(wrapper);
    let encoded = serde_json::to_vec(identity).expect("native resource identity");
    crate::exception::catch_js_throw(|| {
        let identity = scope.root_string_ptr(crate::string::js_string_from_bytes(
            encoded.as_ptr(),
            encoded.len() as u32,
        ));
        let pair = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(2));
        crate::array::js_array_set_f64(pair.get_raw_mut_ptr(), 0, wrapper.get_nanbox_f64());
        crate::array::js_array_set_f64(
            pair.get_raw_mut_ptr(),
            1,
            crate::value::js_nanbox_string(
                identity.get_raw_const_ptr::<crate::string::StringHeader>() as i64,
            ),
        );
        let entries = scope.root_nanbox_f64(list(plan.get_nanbox_f64()));
        let pointer = super::super::array_pointer(entries.get_nanbox_f64())
            .expect("private retention entries");
        let index = crate::array::js_array_length(pointer);
        let grown = crate::array::js_array_set_f64_extend(
            pointer,
            index,
            js_nanbox_pointer(pair.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64),
        );
        crate::array::js_array_set_f64(
            super::super::array_pointer(plan.get_nanbox_f64()).unwrap(),
            0,
            js_nanbox_pointer(grown as i64),
        );
    })
}

pub(in crate::component) fn len(plan: f64) -> u32 {
    crate::array::js_array_length(super::super::array_pointer(list(plan)).unwrap())
}

fn pair(plan: f64, index: u32) -> *mut crate::array::ArrayHeader {
    let entry =
        crate::array::js_array_get_f64(super::super::array_pointer(list(plan)).unwrap(), index);
    super::super::array_pointer(entry).expect("private retention pair")
}

pub(in crate::component) fn identity(plan: f64, index: u32) -> Value {
    let encoded = crate::array::js_array_get_f64(pair(plan, index), 1);
    serde_json::from_str(&string(encoded)).expect("saved native resource identity")
}

pub(in crate::component) fn release(plan: f64, index: u32) {
    let wrapper = crate::array::js_array_get_f64(pair(plan, index), 0);
    super::super::async_value::transferred(wrapper);
    crate::array::js_array_set_f64(pair(plan, index), 0, super::super::undefined());
}

/// Prepare before host ownership validation; release these slots only after
/// that validation succeeds. Matching never reads the wrapper or calls JS.
pub(in crate::component) fn transferred(plan: f64, event: &Value) -> Vec<u32> {
    if event["kind"] != "lower" {
        return Vec::new();
    }
    let mut slots = Vec::new();
    for index in 0..len(plan) {
        let saved = identity(plan, index);
        let matches = if let Some(token) = saved["$hostResource"].as_str() {
            saved["owned"] == true
                && event["transferred"].as_array().is_some_and(|items| {
                    items.iter().any(|item| {
                        item["token"].as_str() == Some(token)
                            && item["type"] == saved["type"]
                            && item["generation"] == saved["generation"]
                    })
                })
        } else if let Some(token) = saved["$resource"].as_str() {
            saved["owned"] == true && contains(&event["guestTransferred"], token)
        } else if let Some(token) = saved["$future"].as_str().or(saved["$stream"].as_str()) {
            contains(&event["asyncTransferred"], token)
        } else {
            false
        };
        if matches {
            slots.push(index);
        }
    }
    slots
}

fn contains(items: &Value, token: &str) -> bool {
    items
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(token)))
}

#[cfg(test)]
mod tests;

/// No allocation or application access occurs while inspecting these slots.
pub(in crate::component) fn is_empty(plan: f64) -> bool {
    (0..len(plan)).all(|index| {
        JSValue::from_bits(crate::array::js_array_get_f64(pair(plan, index), 0).to_bits())
            .is_undefined()
    })
}
