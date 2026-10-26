//! Managed invocation edges and native-only bookkeeping. Private row slots,
//! rather than native JS values or late outer-scope handles, survive callbacks.
use super::*;
use crate::async_hooks::AsyncResourceIds;

pub(super) const PROMISE: u32 = 0;
pub(super) const ARGUMENTS: u32 = 1;
pub(super) const CONTEXT: u32 = 2;
pub(super) const ERROR: u32 = 3;
pub(super) const IMPORT_RESULTS: u32 = 4;
pub(super) const RESULT: u32 = 5;
pub(super) const TARGET: u32 = 6;
const LENGTH: u32 = 7;

#[derive(Clone)]
pub(super) enum ReplyKind {
    Call { interface: String, function: String },
    Unit,
    Collection(Value),
    Read { payload: Value, stream: bool },
}

pub(super) struct Origin {
    pub slot: u32,
    pub ids: AsyncResourceIds,
    pub reply: ReplyKind,
    pub admission: u64,
    // Mark every native reply ready before any settlement hook can reenter.
    pub ready: bool,
    pub public_done: bool,
    pub failed: bool,
    pub error_reported: bool,
    pub synchronous: bool,
    pub retained: bool,
    pub weak_promise: bool,
}

pub(super) fn table(driver_owner: f64) -> f64 {
    payload::state_get(driver_owner, &PUMP, b"rows")
}

pub(super) fn create(promise: f64, arguments: f64, context: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let promise = scope.root_nanbox_f64(promise);
    let arguments = scope.root_nanbox_f64(arguments);
    let context = scope.root_nanbox_f64(context);
    let row = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(LENGTH));
    for (column, value) in [
        promise.get_nanbox_f64(),
        arguments.get_nanbox_f64(),
        context.get_nanbox_f64(),
        undefined(),
        undefined(),
        undefined(),
        undefined(),
    ]
    .into_iter()
    .enumerate()
    {
        crate::array::js_array_set_f64(row.get_raw_mut_ptr(), column as u32, value);
    }
    crate::value::js_nanbox_pointer(row.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64)
}

pub(super) fn row(rows: f64, slot: u32) -> f64 {
    crate::array::js_array_get_f64(array_pointer(rows).expect("private invocation table"), slot)
}

pub(super) fn get(row: f64, column: u32) -> f64 {
    crate::array::js_array_get_f64(
        array_pointer(row).expect("private invocation record"),
        column,
    )
}

pub(super) fn set(row: f64, column: u32, value: f64) {
    crate::array::js_array_set_f64(
        array_pointer(row).expect("private invocation record"),
        column,
        value,
    );
}

pub(super) fn insert(driver_owner: f64, slot: u32, row: f64) {
    let scope = RuntimeHandleScope::new();
    let driver_owner = scope.root_nanbox_f64(driver_owner);
    let row = scope.root_nanbox_f64(row);
    let rows = scope.root_nanbox_f64(table(driver_owner.get_nanbox_f64()));
    let grown = crate::array::js_array_set_f64_extend(
        array_pointer(rows.get_nanbox_f64()).expect("private invocation table"),
        slot,
        row.get_nanbox_f64(),
    );
    // Growth can replace the array header outside a moving collection.
    rows.set_nanbox_f64(crate::value::js_nanbox_pointer(grown as i64));
    payload::state_set(
        driver_owner.get_nanbox_f64(),
        &PUMP,
        b"rows",
        rows.get_nanbox_f64(),
    );
}

pub(super) fn release(rows: f64, slot: u32) {
    crate::array::js_array_set_f64(
        array_pointer(rows).expect("private invocation table"),
        slot,
        undefined(),
    );
}
