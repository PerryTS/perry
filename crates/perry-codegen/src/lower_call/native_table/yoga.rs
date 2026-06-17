//! `perry/yoga` — native taffy-backed flexbox primitives consumed by the
//! `yoga-layout` TS shim (so real `ink` lays out natively). All entries are
//! free functions taking plain f64 args (handles are integer ids, not tagged
//! pointers) and returning an f64 (numbers, or NaN-boxed `undefined` for
//! setters). See `perry-runtime/src/yoga.rs`.

use super::*;

macro_rules! yoga_fn {
    ($method:literal, $runtime:literal, $argc:expr) => {
        NativeModSig {
            module: "perry/yoga",
            has_receiver: false,
            method: $method,
            class_filter: None,
            runtime: $runtime,
            args: &[NA_F64; $argc],
            ret: NR_F64,
        }
    };
}

pub(super) const YOGA_ROWS: &[NativeModSig] = &[
    yoga_fn!("nodeNew", "js_yoga_node_new", 0),
    yoga_fn!("nodeFree", "js_yoga_node_free", 1),
    yoga_fn!("insertChild", "js_yoga_insert_child", 3),
    yoga_fn!("removeChild", "js_yoga_remove_child", 2),
    yoga_fn!("childCount", "js_yoga_child_count", 1),
    yoga_fn!("setMeasureFunc", "js_yoga_set_measure_func", 2),
    yoga_fn!("unsetMeasureFunc", "js_yoga_unset_measure_func", 1),
    yoga_fn!("setNumber", "js_yoga_set_number", 4),
    yoga_fn!("setEdge", "js_yoga_set_edge", 5),
    yoga_fn!("setGap", "js_yoga_set_gap", 4),
    yoga_fn!("setEnum", "js_yoga_set_enum", 3),
    yoga_fn!("calculateLayout", "js_yoga_calculate_layout", 4),
    yoga_fn!("getComputed", "js_yoga_get_computed", 2),
    yoga_fn!("getComputedEdge", "js_yoga_get_computed_edge", 3),
];
