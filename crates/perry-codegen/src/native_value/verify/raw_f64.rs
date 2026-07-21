use super::*;

use crate::native_value::artifact::{NativeFactUse, NativeRepRecord, NativeValueState};
use crate::native_value::buffer::{AliasState, BoundsState, BufferAccessMode};
use crate::native_value::materialize::MaterializationReason;
use crate::native_value::rep::NativeRep;

pub(crate) fn raw_f64_checked_native_consumer(record: &NativeRepRecord) -> bool {
    matches!(
        record.consumer.as_str(),
        "js_array_numeric_get_f64_unboxed"
            | "js_array_numeric_set_f64_unboxed"
            | "js_array_numeric_push_f64_unboxed"
            | "packed_f64_loop_load"
            | "packed_i32_loop_load"
            | "packed_u32_loop_load"
            | "packed_f64_loop_store"
            | "class_field_get.raw_f64_load"
            | "class_field_set.raw_f64_store"
    )
}

pub(crate) fn validate_js_value_bits_record(record: &NativeRepRecord, errors: &mut Vec<String>) {
    if !matches!(record.native_rep, NativeRep::JsValueBits) {
        return;
    }
    let prefix = || {
        format!(
            "{}:{} {}",
            record.function, record.block_label, record.consumer
        )
    };
    if record.native_abi_type.is_some() {
        errors.push(format!(
            "{} js_value_bits cannot be used as an external ABI descriptor",
            prefix()
        ));
    }
    if record.access_mode == Some(BufferAccessMode::DynamicFallback)
        || record.fallback_reason.is_some()
        || record.native_value_state == NativeValueState::DynamicFallback
    {
        errors.push(format!(
            "{} js_value_bits cannot be a dynamic fallback record",
            prefix()
        ));
    }
    if record.materialization_reason.is_some()
        || record.native_value_state == NativeValueState::Materialized
    {
        let transition = record
            .native_abi_transition
            .as_ref()
            .or(record.scalar_conversion.as_ref());
        if !transition.is_some_and(|conversion| {
            valid_native_abi_transition(
                conversion.from_native_rep.as_str(),
                conversion.to_native_rep.as_str(),
                &conversion.op,
                conversion.lossy,
                &record.native_rep,
            )
        }) {
            errors.push(format!(
                "{} materialized js_value_bits record must carry a valid native-to-bits transition",
                prefix()
            ));
        }
    }
}

pub(crate) fn raw_f64_dynamic_fallback_record(record: &NativeRepRecord) -> bool {
    matches!(
        (record.expr_kind.as_str(), record.consumer.as_str()),
        ("NumericArrayPush", "js_array_push_f64")
            | (
                "NumericArrayIndexGet",
                "js_typed_feedback_array_index_get_fallback_boxed"
            )
            | (
                "NumericArrayIndexSet",
                "js_typed_feedback_array_index_set_fallback_boxed"
            )
            | (
                "PackedF64LoopStore",
                "js_typed_feedback_array_index_set_fallback_boxed"
            )
            | ("PackedF64LoopGuard", "packed_f64_loop_fallback")
            | ("PackedI32LoopGuard", "packed_i32_loop_fallback")
            | ("PackedU32LoopGuard", "packed_u32_loop_fallback")
            | ("ClassFieldGet", "js_object_get_field_by_name_f64")
            | ("ClassFieldSet", "js_object_set_field_by_name")
    )
}

pub(crate) fn has_raw_f64_layout_fact(
    facts: &[NativeFactUse],
    state: &str,
    reason: Option<MaterializationReason>,
) -> bool {
    facts.iter().any(|fact| {
        fact.kind == "raw_f64_layout"
            && fact.state == state
            && match reason.as_ref() {
                Some(expected) => fact.reason.as_ref() == Some(expected),
                None => true,
            }
    })
}

pub(crate) fn validate_raw_f64_layout_facts(record: &NativeRepRecord, errors: &mut Vec<String>) {
    if raw_f64_checked_native_consumer(record)
        && !has_raw_f64_layout_fact(&record.consumed_facts, "consumed", None)
    {
        errors.push(format!(
            "{}:{} {} raw-f64 fast path missing consumed raw_f64_layout fact",
            record.function, record.block_label, record.consumer
        ));
    }
    if raw_f64_dynamic_fallback_record(record) {
        if record.materialization_reason.as_ref() != Some(&MaterializationReason::RuntimeApi)
            || record.fallback_reason.as_ref() != Some(&MaterializationReason::RuntimeApi)
        {
            errors.push(format!(
                "{}:{} {} raw-f64 fallback missing runtime_api materialization/fallback reason",
                record.function, record.block_label, record.consumer
            ));
        }
        if !has_raw_f64_layout_fact(
            &record.rejected_facts,
            "rejected",
            Some(MaterializationReason::RuntimeApi),
        ) {
            errors.push(format!(
                "{}:{} {} raw-f64 fallback missing rejected raw_f64_layout fact",
                record.function, record.block_label, record.consumer
            ));
        }
        if !has_raw_f64_layout_fact(
            &record.rejected_facts,
            "invalidated",
            Some(MaterializationReason::RuntimeApi),
        ) {
            errors.push(format!(
                "{}:{} {} raw-f64 fallback missing invalidated raw_f64_layout fact",
                record.function, record.block_label, record.consumer
            ));
        }
    }
}

pub(crate) fn validate_native_owned_unchecked_access(
    record: &NativeRepRecord,
    errors: &mut Vec<String>,
) {
    let Some(fact) = record.native_owned_view.as_ref() else {
        return;
    };
    let prefix = || {
        format!(
            "{}:{} {}",
            record.function, record.block_label, record.consumer
        )
    };
    if fact.owner_root_state != "rooted" {
        errors.push(format!(
            "{} unchecked native-owned view access missing rooted owner",
            prefix()
        ));
    }
    if fact.disposed_state != "alive" {
        errors.push(format!(
            "{} unchecked native-owned view access may use disposed owner",
            prefix()
        ));
    }
    if !matches!(
        record.bounds_state,
        Some(BoundsState::Proven { .. } | BoundsState::Guarded { .. })
    ) {
        errors.push(format!(
            "{} unchecked native-owned view access missing bounds proof",
            prefix()
        ));
    }
    if !matches!(
        record.alias_state,
        Some(AliasState::NoAliasProven | AliasState::NoAliasGuarded { .. })
    ) {
        errors.push(format!(
            "{} unchecked native-owned view access missing alias proof",
            prefix()
        ));
    }
}

pub(crate) fn validate_fact_uses(record: &NativeRepRecord, errors: &mut Vec<String>) {
    for (field, facts) in [
        ("consumed_facts", record.consumed_facts.as_slice()),
        ("rejected_facts", record.rejected_facts.as_slice()),
    ] {
        for fact in facts {
            if fact.fact_id.trim().is_empty() {
                errors.push(format!(
                    "{}:{} {} {field} has empty fact_id",
                    record.function, record.block_label, record.consumer
                ));
            }
            if fact.kind.trim().is_empty() {
                errors.push(format!(
                    "{}:{} {} {field} has empty kind",
                    record.function, record.block_label, record.consumer
                ));
            }
            if fact.state.trim().is_empty() {
                errors.push(format!(
                    "{}:{} {} {field} has empty state",
                    record.function, record.block_label, record.consumer
                ));
            }
            if field == "rejected_facts"
                && fact.reason.is_none()
                && fact.detail.trim().is_empty()
                && !matches!(fact.state.as_str(), "rejected" | "invalidated" | "missing")
            {
                errors.push(format!(
                    "{}:{} {} rejected fact {} lacks reason/detail",
                    record.function, record.block_label, record.consumer, fact.fact_id
                ));
            }
        }
    }
}

fn record_has_note(record: &NativeRepRecord, note: &str) -> bool {
    record.notes.iter().any(|candidate| candidate == note)
}

pub(crate) fn validate_packed_f64_loop_record(record: &NativeRepRecord, errors: &mut Vec<String>) {
    if !matches!(
        record.consumer.as_str(),
        "packed_f64_loop_guard"
            | "packed_f64_loop_load"
            | "packed_f64_loop_store"
            | "packed_i32_loop_guard"
            | "packed_i32_loop_load"
            | "packed_u32_loop_guard"
            | "packed_u32_loop_load"
    ) {
        return;
    }
    for required in ["index_range=nonnegative_i32", "length_range=guarded_i32"] {
        if !record_has_note(record, required) {
            errors.push(format!(
                "{}:{} {} packed-f64 loop access missing {} proof note",
                record.function, record.block_label, record.consumer, required
            ));
        }
    }
    if record.consumer == "packed_f64_loop_store" {
        for required in [
            "rhs_numeric_guard=js_typed_feedback_numeric_array_index_set_guard",
            "raw_f64_canonicalized=js_array_numeric_value_to_raw_f64",
            "array_reloaded_after_rhs=1",
            "array_reloaded_after_store_guard=1",
            "array_reloaded_after_canonicalization=1",
        ] {
            if !record_has_note(record, required) {
                errors.push(format!(
                    "{}:{} {} packed-f64 loop store missing {} safety note",
                    record.function, record.block_label, record.consumer, required
                ));
            }
        }
    }
}

/// #1849 Slice 3 (native-loop region gates): the fast-path packed-numeric loop
/// consumers below run inside a proven hot loop region. Acceptance requires such
/// a region to "prove no unexpected runtime calls" - the guarded fast clone must
/// keep its value region-local and must never materialize back to a JS value or
/// route through a dynamic runtime helper. The dynamic side-exit / fallback
/// consumers (`*_fallback`, `*_store_side_exit`) are the runtime-call boundary
/// itself and are deliberately excluded here; they carry an explicit
/// `RuntimeApi` materialization and are checked by
/// [`raw_f64_dynamic_fallback_record`].
pub(crate) fn packed_loop_region_positive_consumer(consumer: &str) -> bool {
    matches!(
        consumer,
        "packed_f64_loop_guard"
            | "packed_i32_loop_guard"
            | "packed_u32_loop_guard"
            | "packed_f64_loop_load"
            | "packed_i32_loop_load"
            | "packed_u32_loop_load"
            | "packed_i32_loop_load_f64"
            | "packed_u32_loop_load_f64"
            | "packed_f64_loop_store"
            | "packed_i32_loop_store"
            | "packed_u32_loop_store"
    )
}

/// Enforce that a positive packed-loop hot-region record stays call-free: it
/// must remain region-local and carry neither a materialization reason, a
/// fallback reason, nor a dynamic-fallback access mode. Emitters keep these
/// records region-local today; the gate makes that guarantee explicit so a
/// future regression that lets a hot loop iteration escape to a boxed/runtime
/// path (without recording it as a proper side-exit) is rejected.
pub(crate) fn validate_packed_loop_region_call_free(
    record: &NativeRepRecord,
    errors: &mut Vec<String>,
) {
    if !packed_loop_region_positive_consumer(record.consumer.as_str()) {
        return;
    }
    let prefix = || {
        format!(
            "{}:{} {}",
            record.function, record.block_label, record.consumer
        )
    };
    if record.native_value_state != NativeValueState::RegionLocal {
        errors.push(format!(
            "{} packed-loop hot region must stay region-local, found {:?}",
            prefix(),
            record.native_value_state
        ));
    }
    if let Some(reason) = record.materialization_reason.as_ref() {
        errors.push(format!(
            "{} packed-loop hot region emitted an unexpected runtime-call materialization ({:?})",
            prefix(),
            reason
        ));
    }
    if let Some(reason) = record.fallback_reason.as_ref() {
        errors.push(format!(
            "{} packed-loop hot region emitted an unexpected dynamic fallback ({:?})",
            prefix(),
            reason
        ));
    }
    if matches!(
        record.access_mode.as_ref(),
        Some(BufferAccessMode::DynamicFallback)
    ) {
        errors.push(format!(
            "{} packed-loop hot region used a dynamic-fallback access mode",
            prefix()
        ));
    }
}
