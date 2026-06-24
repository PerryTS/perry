use super::*;

use anyhow::{bail, Result};

#[cfg(test)]
use crate::native_value::artifact::NativeAbiTransitionRecord;
use crate::native_value::artifact::{
    NativeAbiDirection, NativeAbiTransitionOp, NativeFactUse, NativeRepRecord, NativeValueState,
    PodLayoutManifest,
};
use crate::native_value::buffer::{AliasState, BoundsState, BufferAccessMode};
use crate::native_value::materialize::MaterializationReason;
use crate::native_value::pod::recompute_layout_from_fields;
use crate::native_value::rep::NativeRep;
use crate::types::{DOUBLE, F32, I32, I64, I8, PTR};

pub(crate) fn raw_f64_checked_native_consumer(record: &NativeRepRecord) -> bool {
    matches!(
        record.consumer.as_str(),
        "js_array_numeric_get_f64_unboxed"
            | "js_array_numeric_set_f64_unboxed"
            | "js_array_numeric_push_f64_unboxed"
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
            conversion.from_native_rep == NativeRep::JsValue.name()
                && conversion.to_native_rep == NativeRep::JsValueBits.name()
                && conversion.op == NativeAbiTransitionOp::JsValueToBits
                && !conversion.lossy
        }) {
            errors.push(format!(
                "{} materialized js_value_bits record must carry js_value_to_bits transition",
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
