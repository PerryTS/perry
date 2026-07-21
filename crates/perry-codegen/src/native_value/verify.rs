use anyhow::{bail, Result};

#[cfg(test)]
use super::artifact::{NativeAbiTransitionOp, NativeAbiTransitionRecord};
use super::artifact::{NativeRepRecord, NativeValueState};
use super::buffer::{AliasState, BoundsState, BufferAccessMode};
#[cfg(test)]
use super::pod::recompute_layout_from_fields;
use super::rep::NativeRep;

mod abi;
mod layout;
mod raw_f64;
#[cfg(test)]
mod tests;

use abi::{
    validate_buffer_span_pairs, validate_native_abi_type_record, validate_pod_view_span_pairs,
};
use layout::{expected_llvm_type, valid_native_abi_transition, validate_pod_layout};
use raw_f64::{
    validate_fact_uses, validate_js_value_bits_record, validate_native_owned_unchecked_access,
    validate_packed_f64_loop_record, validate_packed_loop_region_call_free,
    validate_raw_f64_layout_facts,
};

pub(crate) fn verify_native_rep_records(records: &[NativeRepRecord]) -> Result<()> {
    let mut errors = Vec::new();
    for record in records {
        if let Some(expected_ty) = expected_llvm_type(&record.native_rep) {
            if record.llvm_ty != expected_ty {
                errors.push(format!(
                    "{}:{} {} recorded {} as {}, expected {}",
                    record.function,
                    record.block_label,
                    record.consumer,
                    record.native_rep_name,
                    record.llvm_ty,
                    expected_ty
                ));
            }
        }
        if matches!(record.native_rep, NativeRep::BufferView(_))
            && (record.materialization_reason.is_some()
                || record.fallback_reason.is_some()
                || record.native_value_state != NativeValueState::RegionLocal)
        {
            errors.push(format!(
                "{}:{} {} buffer_view escaped region-local use",
                record.function, record.block_label, record.consumer
            ));
        }
        validate_js_value_bits_record(record, &mut errors);
        if matches!(
            record.native_rep,
            NativeRep::NativeHandle | NativeRep::PromiseBoundary
        ) && (record.materialization_reason.is_some()
            || record.fallback_reason.is_some()
            || record.native_value_state != NativeValueState::RegionLocal)
        {
            errors.push(format!(
                "{}:{} {} {} escaped region-local use",
                record.function, record.block_label, record.consumer, record.native_rep_name
            ));
        }
        if let NativeRep::PodRecord {
            layout_id,
            size,
            alignment,
        } = &record.native_rep
        {
            if record.materialization_reason.is_some()
                || record.fallback_reason.is_some()
                || record.native_value_state != NativeValueState::RegionLocal
            {
                errors.push(format!(
                    "{}:{} {} pod_record escaped region-local use",
                    record.function, record.block_label, record.consumer
                ));
            }
            match record.pod_layout.as_ref() {
                Some(layout)
                    if layout.layout_id == *layout_id
                        && layout.size == *size
                        && layout.alignment == *alignment => {}
                Some(_) => errors.push(format!(
                    "{}:{} {} pod_record manifest does not match native rep",
                    record.function, record.block_label, record.consumer
                )),
                None => errors.push(format!(
                    "{}:{} {} pod_record missing layout manifest",
                    record.function, record.block_label, record.consumer
                )),
            }
        }
        if let NativeRep::PodRecordView {
            layout_id,
            stride,
            alignment,
        } = &record.native_rep
        {
            if record.materialization_reason.is_some()
                || record.fallback_reason.is_some()
                || record.native_value_state != NativeValueState::RegionLocal
            {
                errors.push(format!(
                    "{}:{} {} pod_record_view escaped region-local use",
                    record.function, record.block_label, record.consumer
                ));
            }
            match record.pod_record_view.as_ref() {
                Some(view)
                    if view.layout_id == *layout_id
                        && view.stride == *stride
                        && view.alignment == *alignment
                        && view.pointer_free_backing
                        && view.endian == "native"
                        && view.packing == "c" => {}
                Some(_) => errors.push(format!(
                    "{}:{} {} pod_record_view manifest does not match native rep",
                    record.function, record.block_label, record.consumer
                )),
                None => errors.push(format!(
                    "{}:{} {} pod_record_view missing proof manifest",
                    record.function, record.block_label, record.consumer
                )),
            }
        }
        if let Some(layout) = record.pod_layout.as_ref() {
            validate_pod_layout(layout, record, &mut errors);
        }
        if matches!(record.native_rep, NativeRep::F32)
            && (record.materialization_reason.is_some()
                || record.fallback_reason.is_some()
                || record.native_value_state != NativeValueState::RegionLocal)
        {
            errors.push(format!(
                "{}:{} {} f32 cannot be recorded as JS-visible/materialized",
                record.function, record.block_label, record.consumer
            ));
        }
        if matches!(
            record.access_mode.as_ref(),
            Some(BufferAccessMode::DynamicFallback)
        ) && (record.fallback_reason.is_none() || record.materialization_reason.is_none())
        {
            errors.push(format!(
                "{}:{} {} dynamic fallback missing fallback/materialization reason",
                record.function, record.block_label, record.consumer
            ));
        }
        let transition = record
            .native_abi_transition
            .as_ref()
            .or(record.scalar_conversion.as_ref());
        if let Some(conversion) = transition {
            if record.materialization_reason.is_none() {
                errors.push(format!(
                    "{}:{} {} native ABI transition missing materialization reason",
                    record.function, record.block_label, record.consumer
                ));
            }
            if record.materialization_reason.as_ref() != Some(&conversion.reason) {
                errors.push(format!(
                    "{}:{} {} native ABI transition reason does not match record reason",
                    record.function, record.block_label, record.consumer
                ));
            }
            if !valid_native_abi_transition(
                conversion.from_native_rep.as_str(),
                conversion.to_native_rep.as_str(),
                &conversion.op,
                conversion.lossy,
                &record.native_rep,
            ) {
                errors.push(format!(
                    "{}:{} {} invalid native ABI transition {} -> {} via {:?}",
                    record.function,
                    record.block_label,
                    record.consumer,
                    conversion.from_native_rep,
                    conversion.to_native_rep,
                    conversion.op
                ));
            }
        }
        if let Some(native_abi_type) = record.native_abi_type.as_ref() {
            validate_native_abi_type_record(record, native_abi_type, &mut errors);
        }
        if record.emitted_inbounds
            && !matches!(
                record.bounds_state,
                Some(BoundsState::Proven { .. } | BoundsState::Guarded { .. })
            )
        {
            errors.push(format!(
                "{}:{} {} emitted inbounds without proven/guarded bounds",
                record.function, record.block_label, record.consumer
            ));
        }
        if record.emitted_noalias
            && !matches!(
                record.alias_state,
                Some(AliasState::NoAliasProven | AliasState::NoAliasGuarded { .. })
            )
        {
            errors.push(format!(
                "{}:{} {} emitted noalias without proven/guarded alias state",
                record.function, record.block_label, record.consumer
            ));
        }
        if record
            .bounds_state
            .as_ref()
            .is_some_and(BoundsState::uses_unsound_explicit_assume_guard)
        {
            errors.push(format!(
                "{}:{} {} used explicit_assume as a bounds guard without a source proof",
                record.function, record.block_label, record.consumer
            ));
        }
        if matches!(
            record.access_mode.as_ref(),
            Some(BufferAccessMode::UncheckedNative)
        ) && !matches!(
            record.bounds_state,
            Some(BoundsState::Proven { .. } | BoundsState::Guarded { .. })
        ) {
            errors.push(format!(
                "{}:{} {} used unchecked native buffer access without proven/guarded bounds",
                record.function, record.block_label, record.consumer
            ));
        }
        if matches!(
            record.access_mode.as_ref(),
            Some(BufferAccessMode::UncheckedNative)
        ) && record.native_owned_view.is_some()
        {
            validate_native_owned_unchecked_access(record, &mut errors);
        }
        if matches!(
            record.access_mode.as_ref(),
            Some(BufferAccessMode::CheckedNative)
        ) && !matches!(
            record.bounds_state,
            Some(BoundsState::Proven { .. } | BoundsState::Guarded { .. })
        ) {
            errors.push(format!(
                "{}:{} {} used checked native buffer access without proven/guarded bounds",
                record.function, record.block_label, record.consumer
            ));
        }
        validate_fact_uses(record, &mut errors);
        validate_raw_f64_layout_facts(record, &mut errors);
        validate_packed_f64_loop_record(record, &mut errors);
        validate_packed_loop_region_call_free(record, &mut errors);
    }
    validate_buffer_span_pairs(records, &mut errors);
    validate_pod_view_span_pairs(records, &mut errors);
    if !errors.is_empty() {
        bail!(
            "native representation verifier failed: {}",
            errors.join("; ")
        );
    }
    Ok(())
}
