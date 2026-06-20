#[cfg(feature = "diagnostics")]
use std::path::{Path, PathBuf};

use super::*;

fn observation_source_name(source: ObservationSource) -> &'static str {
    match source {
        ObservationSource::Property => "property",
        ObservationSource::Method => "method",
        ObservationSource::Closure => "closure",
        ObservationSource::Array => "array",
        ObservationSource::NumericWrite => "numeric_write",
        ObservationSource::HelperReturn => "helper_return",
    }
}

fn heap_type_name(heap_type: u16) -> &'static str {
    match heap_type as u8 {
        0 => "unknown",
        crate::gc::GC_TYPE_ARRAY => "array",
        crate::gc::GC_TYPE_OBJECT => "object",
        crate::gc::GC_TYPE_STRING => "string",
        crate::gc::GC_TYPE_CLOSURE => "closure",
        crate::gc::GC_TYPE_PROMISE => "promise",
        crate::gc::GC_TYPE_BIGINT => "bigint",
        crate::gc::GC_TYPE_ERROR => "error",
        crate::gc::GC_TYPE_MAP => "map",
        crate::gc::GC_TYPE_LAZY_ARRAY => "lazy_array",
        crate::gc::GC_TYPE_BUFFER => "buffer",
        crate::gc::GC_TYPE_TYPED_ARRAY => "typed_array",
        crate::gc::GC_TYPE_SET => "set",
        _ => "unknown",
    }
}

fn stable_value_kind_name(kind: u16) -> String {
    let name = match kind {
        STABLE_VALUE_NUMBER => "number",
        STABLE_VALUE_BOOLEAN => "boolean",
        STABLE_VALUE_NULL => "null",
        STABLE_VALUE_UNDEFINED => "undefined",
        STABLE_VALUE_HOLE => "hole",
        STABLE_VALUE_SHORT_STRING => "short_string",
        STABLE_VALUE_STRING => "string",
        STABLE_VALUE_BIGINT => "bigint",
        STABLE_VALUE_POINTER => "pointer",
        STABLE_VALUE_INT32 => "int32",
        STABLE_VALUE_JS_HANDLE => "js_handle",
        _ if kind < 0x7ff8 => "number",
        _ if kind == (POINTER_TAG >> 48) as u16 => "pointer",
        _ if kind == (STRING_TAG >> 48) as u16 => "string",
        _ if kind == (BIGINT_TAG >> 48) as u16 => "bigint",
        _ if kind == (SHORT_STRING_TAG >> 48) as u16 => "short_string",
        _ if kind == (INT32_TAG >> 48) as u16 => "int32",
        _ if kind == (JS_HANDLE_TAG >> 48) as u16 => "js_handle",
        _ => return format!("raw_tag_0x{kind:04x}"),
    };
    name.to_string()
}

fn array_access_kind_name(kind: u8) -> &'static str {
    match kind {
        ARRAY_ACCESS_INDEXED_IN_BOUNDS => "indexed_in_bounds",
        ARRAY_ACCESS_INDEXED_OUT_OF_BOUNDS => "indexed_out_of_bounds",
        ARRAY_ACCESS_STRING_KEY => "string_key",
        _ => "unknown",
    }
}

fn array_layout_kind_name(kind: u8) -> &'static str {
    match kind {
        ARRAY_LAYOUT_EMPTY => "empty",
        ARRAY_LAYOUT_POINTER_FREE => "pointer_free",
        ARRAY_LAYOUT_POINTER_ONLY => "pointer_only",
        ARRAY_LAYOUT_MIXED => "mixed",
        ARRAY_LAYOUT_UNKNOWN => "unknown",
        ARRAY_LAYOUT_BUFFER => "buffer",
        ARRAY_LAYOUT_TYPED_ARRAY => "typed_array",
        ARRAY_LAYOUT_LAZY => "lazy",
        _ => "invalid",
    }
}

fn decode_array_aux(aux: u64) -> (u8, u8, u16, u8) {
    (
        (aux & 0xff) as u8,
        ((aux >> 8) & 0xff) as u8,
        ((aux >> 16) & 0xffff) as u16,
        ((aux >> 32) & 0xff) as u8,
    )
}

fn observation_has_array_facts(obs: &Observation) -> bool {
    matches!(obs.source, ObservationSource::Array)
        || matches!(
            obs.heap_type as u8,
            crate::gc::GC_TYPE_ARRAY
                | crate::gc::GC_TYPE_LAZY_ARRAY
                | crate::gc::GC_TYPE_BUFFER
                | crate::gc::GC_TYPE_TYPED_ARRAY
        )
}

fn observed_kind_json(obs: &Observation) -> serde_json::Value {
    let mut row = serde_json::Map::new();
    row.insert(
        "source".to_string(),
        serde_json::Value::String(observation_source_name(obs.source).to_string()),
    );
    row.insert(
        "heap_type".to_string(),
        serde_json::Value::String(heap_type_name(obs.heap_type).to_string()),
    );
    row.insert("class_id".to_string(), serde_json::json!(obs.class_id));
    row.insert(
        "value_kind".to_string(),
        serde_json::Value::String(stable_value_kind_name(obs.value_tag)),
    );

    if matches!(
        obs.source,
        ObservationSource::Property | ObservationSource::Method | ObservationSource::NumericWrite
    ) {
        row.insert("key_hash".to_string(), serde_json::json!(obs.key_hash));
    }

    if observation_has_array_facts(obs) {
        let (access_kind, layout_kind, element_kind, typed_kind) = decode_array_aux(obs.aux);
        row.insert(
            "array_access".to_string(),
            serde_json::Value::String(array_access_kind_name(access_kind).to_string()),
        );
        row.insert(
            "array_layout".to_string(),
            serde_json::Value::String(array_layout_kind_name(layout_kind).to_string()),
        );
        row.insert(
            "array_element_kind".to_string(),
            serde_json::Value::String(stable_value_kind_name(element_kind)),
        );
        if layout_kind == ARRAY_LAYOUT_TYPED_ARRAY {
            row.insert(
                "typed_array_kind".to_string(),
                serde_json::Value::String(crate::typedarray::name_for_kind(typed_kind).to_string()),
            );
        }
    }

    if matches!(obs.source, ObservationSource::NumericWrite)
        || (matches!(obs.source, ObservationSource::Property) && obs.aux != 0)
    {
        row.insert("field_index".to_string(), serde_json::json!(obs.aux));
    }

    serde_json::Value::Object(row)
}

fn observed_kinds_snapshot(observations: &[Observation]) -> Vec<serde_json::Value> {
    let mut rows = observations
        .iter()
        .map(observed_kind_json)
        .collect::<Vec<serde_json::Value>>();
    rows.sort_by(|a, b| {
        let a = serde_json::to_string(a).unwrap_or_default();
        let b = serde_json::to_string(b).unwrap_or_default();
        a.cmp(&b)
    });
    rows
}

pub fn typed_feedback_snapshot() -> TypedFeedbackSnapshot {
    let reg = registry();
    let mut snapshot = TypedFeedbackSnapshot {
        total_sites: reg.sites.len(),
        shape_invalidations: reg.shape_invalidations,
        method_invalidations: reg.method_invalidations,
        representation_invalidations: reg.representation_invalidations,
        ..TypedFeedbackSnapshot::default()
    };
    let mut rows = Vec::with_capacity(reg.sites.len());
    for site in reg.sites.values() {
        let state = site.state();
        *snapshot
            .by_kind
            .entry(site.metadata.kind.as_str().to_string())
            .or_insert(0) += 1;
        *snapshot
            .by_state
            .entry(state.as_str().to_string())
            .or_insert(0) += 1;
        rows.push(TypedFeedbackSiteSnapshot {
            site_id: site.site_id,
            kind: site.metadata.kind.as_str(),
            state: state.as_str(),
            module: site.metadata.module.clone(),
            function: site.metadata.function.clone(),
            source_label: site.metadata.source_label.clone(),
            operation: site.metadata.operation.clone(),
            guard_name: site.metadata.guard_name.clone(),
            fallback_name: site.metadata.fallback_name.clone(),
            observed_count: site.observed_count,
            observation_count: site.observations.len(),
            guard_passes: site.guard_passes,
            guard_failures: site.guard_failures,
            fallback_calls: site.fallback_calls,
            shape_invalidations: site.shape_invalidations,
            method_invalidations: site.method_invalidations,
            representation_invalidations: site.representation_invalidations,
            observed_kinds: observed_kinds_snapshot(&site.observations),
        });
        snapshot.guard_passes = snapshot.guard_passes.saturating_add(site.guard_passes);
        snapshot.guard_failures = snapshot.guard_failures.saturating_add(site.guard_failures);
        snapshot.fallback_calls = snapshot.fallback_calls.saturating_add(site.fallback_calls);
        snapshot
            .guards_by_name
            .entry(site.metadata.guard_name.clone())
            .or_insert(GuardCounterSnapshot {
                passes: 0,
                failures: 0,
                fallback_calls: 0,
            })
            .add_site(site);
    }
    rows.sort_by_key(|row| row.site_id);
    snapshot.sites = rows;
    snapshot
}

#[cfg(feature = "diagnostics")]
pub fn typed_feedback_trace_json() -> serde_json::Value {
    let snapshot = typed_feedback_snapshot();
    serde_json::json!({
        "total_sites": snapshot.total_sites,
        "by_kind": snapshot.by_kind,
        "by_state": snapshot.by_state,
        "invalidations": {
            "shape": snapshot.shape_invalidations,
            "method": snapshot.method_invalidations,
            "representation": snapshot.representation_invalidations,
        },
        "guards": {
            "passes": snapshot.guard_passes,
            "failures": snapshot.guard_failures,
            "fallback_calls": snapshot.fallback_calls,
            "by_guard": snapshot.guards_by_name.iter().map(|(name, counters)| {
                (
                    name.clone(),
                    serde_json::json!({
                        "passes": counters.passes,
                        "failures": counters.failures,
                        "fallback_calls": counters.fallback_calls,
                    }),
                )
            }).collect::<serde_json::Map<String, serde_json::Value>>(),
        },
        "sites": snapshot.sites.iter().map(|site| {
            serde_json::json!({
                "site_id": site.site_id,
                "kind": site.kind,
                "state": site.state,
                "module": site.module,
                "function": site.function,
                "source_label": site.source_label,
                "operation": site.operation,
                "guard_name": site.guard_name,
                "fallback_name": site.fallback_name,
                "observed_count": site.observed_count,
                "observation_count": site.observation_count,
                "guard_passes": site.guard_passes,
                "guard_failures": site.guard_failures,
                "fallback_calls": site.fallback_calls,
                "observed_kinds": site.observed_kinds.clone(),
                "guards": {
                    "passes": site.guard_passes,
                    "failures": site.guard_failures,
                    "fallback_calls": site.fallback_calls,
                },
                "invalidations": {
                    "shape": site.shape_invalidations,
                    "method": site.method_invalidations,
                    "representation": site.representation_invalidations,
                },
            })
        }).collect::<Vec<_>>(),
    })
}

#[cfg(feature = "diagnostics")]
fn typed_feedback_trace_path_from_env() -> Option<PathBuf> {
    let value = std::env::var("PERRY_TYPED_FEEDBACK_TRACE").ok()?;
    if value.is_empty() || value == "0" {
        return None;
    }
    if value == "1" {
        Some(PathBuf::from("typed-feedback-trace.json"))
    } else {
        Some(PathBuf::from(value))
    }
}

#[cfg(feature = "diagnostics")]
fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

// The extern symbol must ALWAYS be compiled: codegen emits an unconditional
// call to it from `main` (the program-exit trace-dump hook), so gating the
// whole function breaks the final link under auto-optimize (`Undefined symbols:
// _js_typed_feedback_maybe_dump_trace`). Only the JSON-building body is gated
// behind `diagnostics`; with the feature off this is a no-op (the
// `PERRY_TYPED_FEEDBACK` trace is a dev diagnostic, absent from size-optimized
// binaries).
#[no_mangle]
pub extern "C" fn js_typed_feedback_maybe_dump_trace() {
    #[cfg(feature = "diagnostics")]
    {
        let Some(path) = typed_feedback_trace_path_from_env() else {
            return;
        };
        if TRACE_DUMPED.swap(true, Ordering::AcqRel) {
            return;
        }

        let json = typed_feedback_trace_json();
        let bytes = match serde_json::to_vec_pretty(&json) {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("perry typed-feedback trace: failed to encode JSON: {err}");
                return;
            }
        };
        if let Err(err) = ensure_parent_dir(&path).and_then(|_| std::fs::write(&path, bytes)) {
            eprintln!(
                "perry typed-feedback trace: failed to write {}: {err}",
                path.display()
            );
        }
    }
}

// #1752: codegen emits calls to these `js_typed_feedback_*` instrumentation
// helpers (property-get/set, method-call, array index, guard recording, etc.)
// for sites it decides to profile. Nothing in the Rust crate graph references
// them, so the default `.a` keeps them via staticlib-export semantics — but
// the auto-optimize build round-trips the runtime through whole-program LLVM
// bitcode and is free to internalize + dead-strip an unreferenced `#[no_mangle]`
// symbol, leaving the codegen call dangling (`Undefined symbols:
// _js_typed_feedback_native_call_method` etc. at final link — which is exactly
// how an instrumented async program failed to link under auto-optimize). The
// `#[used]` typed fn-pointer statics below take the address of each helper,
// landing the functions themselves in `@llvm.used` so thin-LTO keeps them
// external (not internalized) and the linker's `-dead_strip` honors them — the
// same proven retention mechanism as `value/dyn_index.rs` / `process.rs`
// (#1344). NOTE: a `[usize; N]` (ptrtoint) array or a `[*const (); N]` pointer
// array do NOT keep the symbols external here — only individual typed
// fn-pointer statics survive the thin-LTO + `strip=true` release profile (both
// array forms were verified failing under auto-optimize). Function-pointer
// types are `Sync`, so no wrapper is needed.
#[rustfmt::skip]
mod keep_typed_feedback {
    use super::*;
    use crate::typed_feedback::guards::{
        js_typed_feedback_native_call_method_apply_by_id,
        js_typed_feedback_native_call_method_by_id,
    };
    #[used] static K00: extern "C" fn(u64, u32, *const u8, usize, *const u8, usize, *const u8, usize, *const u8, usize, *const u8, usize, *const u8, usize) = js_typed_feedback_register_site;
    #[used] static K01: extern "C" fn(u64) = js_typed_feedback_record_guard_pass;
    #[used] static K02: extern "C" fn(u64) = js_typed_feedback_record_guard_fail;
    #[used] static K03: extern "C" fn(u64) = js_typed_feedback_record_fallback_call;
    #[used] static K04: extern "C" fn(u64, *const ObjectHeader, *const crate::StringHeader) = js_typed_feedback_observe_property_get;
    #[used] static K05: extern "C" fn(u64, *mut ObjectHeader, *const crate::StringHeader) = js_typed_feedback_observe_property_set;
    #[used] static K06: extern "C" fn(u64, *const ObjectHeader, *const crate::StringHeader) -> f64 = js_typed_feedback_object_get_field_by_name_f64;
    #[used] static K07: extern "C" fn(u64, *mut ObjectHeader, *const crate::StringHeader, f64) = js_typed_feedback_object_set_field_by_name;
    #[used] static K08: extern "C" fn(u64, *mut ObjectHeader, *const crate::StringHeader, f64) = js_typed_feedback_object_set_field_by_name_fast;
    #[used] static K09: unsafe extern "C" fn(u64, f64, *const i8, usize, *const f64, usize) -> f64 = js_typed_feedback_native_call_method;
    #[used] static K10: unsafe extern "C" fn(u64, f64, *const i8, usize, i64) -> f64 = js_typed_feedback_native_call_method_apply;
    #[used] static K11: extern "C" fn(u64, *const ArrayHeader, u32) -> f64 = js_typed_feedback_array_get_f64;
    #[used] static K12: extern "C" fn(u64, f64, i32, i32) -> i32 = js_typed_feedback_plain_array_index_get_guard;
    #[used] static K13: extern "C" fn(u64, f64, i32, i32) -> i32 = js_typed_feedback_numeric_array_index_get_guard;
    #[used] static K14: extern "C" fn(u64, f64) -> i32 = js_typed_feedback_packed_f64_array_loop_guard;
    #[used] static K15: extern "C" fn(u64, f64) -> i32 = js_typed_feedback_packed_u32_array_loop_guard;
    #[used] static K16: extern "C" fn(u64, f64, f64) -> f64 = js_typed_feedback_array_index_get_fallback_boxed;
    #[used] static K17: extern "C" fn(u64, *mut ArrayHeader, u32, f64) = js_typed_feedback_array_set_f64;
    #[used] static K18: extern "C" fn(u64, *mut ArrayHeader, u32, f64) -> *mut ArrayHeader = js_typed_feedback_array_set_f64_extend;
    #[used] static K19: extern "C" fn(u64, f64, i32, f64, i32) -> i32 = js_typed_feedback_plain_array_index_set_guard;
    #[used] static K20: extern "C" fn(u64, f64, i32, f64, i32) -> i32 = js_typed_feedback_numeric_array_index_set_guard;
    #[used] static K21: extern "C" fn(u64, f64, f64) -> i32 = js_typed_feedback_numeric_array_push_guard;
    #[used] static K22: extern "C" fn(u64, f64, f64, f64) -> f64 = js_typed_feedback_array_index_set_fallback_boxed;
    #[used] static K23: extern "C" fn(u64, *const ArrayHeader, u32) = js_typed_feedback_observe_array_element;
    #[used] static K24: extern "C" fn(u64, *mut ArrayHeader, *const crate::StringHeader, f64) -> *mut ArrayHeader = js_typed_feedback_array_set_string_key;
    #[used] static K25: extern "C" fn(u64, *mut ArrayHeader, f64, f64) -> *mut ArrayHeader = js_typed_feedback_array_set_index_or_string;
    #[used] static K26: extern "C" fn(u64, i64, f64, f64) = js_typed_feedback_object_set_index_polymorphic;
    #[used] static K27: extern "C" fn(u64, *mut ObjectHeader, u32, *const crate::StringHeader, f64) = js_typed_feedback_object_set_unboxed_f64_field;
    #[used] static K28: extern "C" fn(u64, f64) -> f64 = js_typed_feedback_observe_helper_return;
    #[used] static K29: extern "C" fn() = js_typed_feedback_maybe_dump_trace;
    #[used] static K30: unsafe extern "C" fn(u64, f64, i64, *const f64, usize) -> f64 = js_typed_feedback_native_call_method_by_id;
    #[used] static K31: unsafe extern "C" fn(u64, f64, i64, i64) -> f64 = js_typed_feedback_native_call_method_apply_by_id;
}
