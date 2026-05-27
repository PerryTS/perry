use anyhow::{bail, Result};

#[cfg(test)]
use super::artifact::NativeAbiTransitionRecord;
use super::artifact::{
    NativeAbiDirection, NativeAbiTransitionOp, NativeFactUse, NativeRepRecord, NativeValueState,
    PodLayoutManifest,
};
use super::buffer::{AliasState, BoundsState, BufferAccessMode};
use super::materialize::MaterializationReason;
use super::pod::recompute_layout_from_fields;
use super::rep::NativeRep;
use crate::types::{DOUBLE, F32, I32, I64, I8, PTR};

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
        validate_raw_f64_layout_facts(record, &mut errors);
    }
    if !errors.is_empty() {
        bail!(
            "native representation verifier failed: {}",
            errors.join("; ")
        );
    }
    Ok(())
}

fn raw_f64_checked_native_consumer(record: &NativeRepRecord) -> bool {
    matches!(
        record.consumer.as_str(),
        "js_array_numeric_get_f64_unboxed"
            | "js_array_numeric_set_f64_unboxed"
            | "js_array_numeric_push_f64_unboxed"
            | "class_field_get.raw_f64_load"
            | "class_field_set.raw_f64_store"
    )
}

fn raw_f64_dynamic_fallback_record(record: &NativeRepRecord) -> bool {
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

fn has_raw_f64_layout_fact(
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

fn validate_raw_f64_layout_facts(record: &NativeRepRecord, errors: &mut Vec<String>) {
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

fn validate_native_owned_unchecked_access(record: &NativeRepRecord, errors: &mut Vec<String>) {
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

fn validate_native_abi_type_record(
    record: &NativeRepRecord,
    abi: &super::artifact::NativeAbiTypeRecord,
    errors: &mut Vec<String>,
) {
    let prefix = || {
        format!(
            "{}:{} {}",
            record.function, record.block_label, record.consumer
        )
    };
    if abi.display.is_empty() || abi.canonical_kind.is_empty() {
        errors.push(format!("{} native ABI descriptor is empty", prefix()));
    }
    match abi.direction {
        NativeAbiDirection::Param => {
            if abi.js_argument_index.is_none() {
                errors.push(format!(
                    "{} native ABI param missing JS argument index",
                    prefix()
                ));
            }
        }
        NativeAbiDirection::Return => {
            if abi.js_argument_index.is_some() {
                errors.push(format!(
                    "{} native ABI return must not carry JS argument index",
                    prefix()
                ));
            }
            if abi.canonical_kind == "buffer+len" {
                errors.push(format!("{} buffer+len cannot be a return type", prefix()));
            }
        }
    }
    if abi.abi_slot_count == 0 && abi.canonical_kind != "void" {
        errors.push(format!("{} native ABI slot count is zero", prefix()));
    }
    if abi.canonical_kind == "handle" {
        match abi.native_handle.as_ref() {
            Some(handle) => {
                if handle.direction != abi.direction
                    || handle.js_argument_index != abi.js_argument_index
                    || handle.abi_slot_index != abi.abi_slot_index
                    || handle.abi_slot_count != abi.abi_slot_count
                {
                    errors.push(format!(
                        "{} native handle contract slot metadata does not match ABI record",
                        prefix()
                    ));
                }
                if handle.type_id == 0 {
                    errors.push(format!("{} native handle type id is zero", prefix()));
                }
                if handle.debug_name.is_empty() {
                    errors.push(format!("{} native handle debug name is empty", prefix()));
                }
                if !matches!(handle.ownership.as_str(), "owned" | "borrowed") {
                    errors.push(format!("{} native handle ownership is invalid", prefix()));
                }
                if !matches!(handle.thread_affinity.as_str(), "any" | "main" | "creator") {
                    errors.push(format!(
                        "{} native handle thread affinity is invalid",
                        prefix()
                    ));
                }
                if handle.has_finalizer != handle.finalizer_symbol.is_some() {
                    errors.push(format!(
                        "{} native handle finalizer presence is inconsistent",
                        prefix()
                    ));
                }
                if handle.has_finalizer && handle.ownership != "owned" {
                    errors.push(format!(
                        "{} native handle finalizer requires owned ownership",
                        prefix()
                    ));
                }
                if handle.has_finalizer && abi.direction == NativeAbiDirection::Param {
                    errors.push(format!(
                        "{} native handle param must not carry a finalizer",
                        prefix()
                    ));
                }
            }
            None => errors.push(format!(
                "{} handle ABI missing native_handle contract",
                prefix()
            )),
        }
    } else if abi.native_handle.is_some() {
        errors.push(format!(
            "{} non-handle ABI must not carry native_handle contract",
            prefix()
        ));
    }
    let rep_matches = match abi.canonical_kind.as_str() {
        "jsvalue" => matches!(&record.native_rep, NativeRep::JsValue),
        "string" | "ptr" | "i64_str" => {
            matches!(
                &record.native_rep,
                NativeRep::NativeHandle | NativeRep::JsValue
            )
        }
        "bool" | "i32" => matches!(&record.native_rep, NativeRep::I32),
        "i64" => matches!(&record.native_rep, NativeRep::I64),
        "u32" => matches!(&record.native_rep, NativeRep::U32),
        "u64" => matches!(&record.native_rep, NativeRep::U64),
        "usize" => matches!(&record.native_rep, NativeRep::USize),
        "f32" => matches!(&record.native_rep, NativeRep::F32),
        "f64" => matches!(&record.native_rep, NativeRep::F64 | NativeRep::JsValue),
        "buffer_len" => matches!(&record.native_rep, NativeRep::BufferLen),
        "buffer+len" => matches!(
            &record.native_rep,
            NativeRep::BufferView(_) | NativeRep::USize | NativeRep::BufferLen
        ),
        "handle" => matches!(&record.native_rep, NativeRep::NativeHandle),
        "promise" => matches!(&record.native_rep, NativeRep::PromiseBoundary),
        "void" => false,
        _ => false,
    };
    if !rep_matches {
        errors.push(format!(
            "{} native ABI descriptor {} does not match recorded native rep {}",
            prefix(),
            abi.display,
            record.native_rep_name
        ));
    }
}

fn expected_llvm_type(rep: &NativeRep) -> Option<&'static str> {
    Some(match rep {
        NativeRep::JsValue | NativeRep::F64 => DOUBLE,
        NativeRep::F32 => F32,
        NativeRep::I64
        | NativeRep::U64
        | NativeRep::USize
        | NativeRep::NativeHandle
        | NativeRep::PromiseBoundary => I64,
        NativeRep::I32 | NativeRep::U32 => I32,
        NativeRep::BufferLen => I32,
        NativeRep::U8 => I8,
        NativeRep::BufferView(_) => PTR,
        NativeRep::PodRecord { .. } => PTR,
    })
}

fn validate_pod_layout(
    layout: &PodLayoutManifest,
    record: &NativeRepRecord,
    errors: &mut Vec<String>,
) {
    let prefix = || {
        format!(
            "{}:{} {}",
            record.function, record.block_label, record.consumer
        )
    };
    if layout.endian != "native" {
        errors.push(format!("{} pod layout has non-native endian", prefix()));
    }
    if layout.packing != "c" {
        errors.push(format!("{} pod layout has non-c packing", prefix()));
    }
    let specs: Vec<(String, NativeRep)> = layout
        .fields
        .iter()
        .map(|field| (field.name.clone(), field.native_rep.clone()))
        .collect();
    let recomputed = match recompute_layout_from_fields(layout.layout_id.clone(), &specs) {
        Ok(layout) => layout,
        Err(reason) => {
            errors.push(format!(
                "{} pod layout recompute failed: {}",
                prefix(),
                reason
            ));
            return;
        }
    };
    if layout.size != recomputed.size || layout.alignment != recomputed.alignment {
        errors.push(format!(
            "{} pod layout size/alignment mismatch recorded=({},{}) recomputed=({},{})",
            prefix(),
            layout.size,
            layout.alignment,
            recomputed.size,
            recomputed.alignment
        ));
    }
    if layout.tail_padding != recomputed.tail_padding {
        errors.push(format!(
            "{} pod layout tail padding mismatch recorded={} recomputed={}",
            prefix(),
            layout.tail_padding,
            recomputed.tail_padding
        ));
    }
    if layout.padding != recomputed.padding {
        errors.push(format!("{} pod layout padding mismatch", prefix()));
    }
    if layout.fields.len() != recomputed.fields.len() {
        errors.push(format!("{} pod layout field count mismatch", prefix()));
        return;
    }
    let mut ranges = Vec::with_capacity(layout.fields.len());
    for (field, expected) in layout.fields.iter().zip(recomputed.fields.iter()) {
        if field.name != expected.name
            || field.native_rep != expected.native_rep
            || field.native_rep_name != field.native_rep.name()
            || field.offset != expected.offset
            || field.size != expected.size
            || field.alignment != expected.alignment
            || field.padding_before != expected.padding_before
        {
            errors.push(format!(
                "{} pod field layout mismatch for {}",
                prefix(),
                field.name
            ));
        }
        if field.offset % field.alignment != 0 {
            errors.push(format!(
                "{} pod field {} offset {} violates alignment {}",
                prefix(),
                field.name,
                field.offset,
                field.alignment
            ));
        }
        ranges.push((
            field.offset,
            field.offset.saturating_add(field.size),
            &field.name,
        ));
    }
    ranges.sort_by_key(|(start, _, _)| *start);
    for pair in ranges.windows(2) {
        let (a_start, a_end, a_name) = pair[0];
        let (b_start, _, b_name) = pair[1];
        if a_end > b_start {
            errors.push(format!(
                "{} pod fields overlap: {}@{}..{} and {}@{}",
                prefix(),
                a_name,
                a_start,
                a_end,
                b_name,
                b_start
            ));
        }
    }
    let pointer_mask_nonzero = layout.pointer_mask.iter().any(|word| *word != 0);
    if pointer_mask_nonzero && !layout.explicit_pointer_metadata {
        errors.push(format!(
            "{} pod layout has nonzero pointer mask without explicit metadata",
            prefix()
        ));
    }
}

fn valid_native_abi_transition(
    from: &str,
    to: &str,
    op: &NativeAbiTransitionOp,
    lossy: bool,
    record_rep: &NativeRep,
) -> bool {
    if to != NativeRep::JsValue.name() {
        return false;
    }
    if !matches!(record_rep, NativeRep::JsValue) {
        return false;
    }
    match op {
        NativeAbiTransitionOp::None => matches!(from, "f64" | "js_value") && !lossy,
        NativeAbiTransitionOp::SignedIntToFloat => {
            matches!(from, "i32" | "i64") && lossy == (from == "i64")
        }
        NativeAbiTransitionOp::UnsignedIntToFloat => {
            matches!(from, "u8" | "u32" | "u64" | "usize" | "buffer_len")
                && lossy == matches!(from, "u64" | "usize")
        }
        NativeAbiTransitionOp::FloatExtend => from == "f32" && !lossy,
        NativeAbiTransitionOp::PointerBox => from == "native_handle" && !lossy,
        NativeAbiTransitionOp::NativeHandleBox => from == "native_handle" && !lossy,
        NativeAbiTransitionOp::PromiseBox => from == "promise_boundary" && !lossy,
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeAbiTransitionOp, NativeAbiTransitionRecord};
    use crate::native_value::{
        verify_native_rep_records, AliasState, BoundsProof, BoundsState, BufferAccessMode,
        BufferViewRep, LoweredValue, MaterializationReason, NativeAbiDirection,
        NativeAbiTypeRecord, NativeFactUse, NativeRep, NativeRepRecord, NativeValueState,
        SemanticKind,
    };
    use crate::types::{DOUBLE, F32, I32, I64, PTR};

    fn record() -> NativeRepRecord {
        let lowered = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::I32,
            llvm_ty: I32,
            value: "%r1".to_string(),
        };
        NativeRepRecord {
            function: "f".to_string(),
            block_label: "entry".to_string(),
            region_id: None,
            source_function: "f".to_string(),
            lowering_block: "entry".to_string(),
            local_id: None,
            expr_kind: "test".to_string(),
            source_key: None,
            semantic: lowered.semantic,
            native_rep_name: lowered.rep.name().to_string(),
            native_rep: lowered.rep,
            llvm_ty: lowered.llvm_ty,
            llvm_value: lowered.value,
            consumer: "test".to_string(),
            bounds_state: None,
            alias_state: None,
            access_mode: None,
            buffer_access: None,
            native_owned_view: None,
            materialization_reason: None,
            fallback_reason: None,
            native_value_state: NativeValueState::RegionLocal,
            native_abi_transition: None,
            scalar_conversion: None,
            native_abi_type: None,
            pod_layout: None,
            consumed_facts: Vec::new(),
            rejected_facts: Vec::new(),
            emitted_inbounds: false,
            emitted_noalias: false,
            notes: Vec::new(),
        }
    }

    fn raw_f64_layout_fact(state: &str, reason: Option<MaterializationReason>) -> NativeFactUse {
        NativeFactUse {
            fact_id: format!("test.raw_f64_layout.{state}"),
            kind: "raw_f64_layout".to_string(),
            local_id: None,
            state: state.to_string(),
            reason,
        }
    }

    fn pod_layout() -> crate::native_value::PodLayoutManifest {
        super::recompute_layout_from_fields(
            "pod_test".to_string(),
            &[
                ("tag".to_string(), NativeRep::U32),
                ("gain".to_string(), NativeRep::F32),
                ("total".to_string(), NativeRep::F64),
                ("count".to_string(), NativeRep::BufferLen),
            ],
        )
        .unwrap()
    }

    fn pod_record(layout: crate::native_value::PodLayoutManifest) -> NativeRepRecord {
        let mut r = record();
        r.semantic = SemanticKind::PodRecord;
        r.native_rep = NativeRep::PodRecord {
            layout_id: layout.layout_id.clone(),
            size: layout.size,
            alignment: layout.alignment,
        };
        r.native_rep_name = "pod_record".to_string();
        r.llvm_ty = PTR;
        r.llvm_value = "%pod".to_string();
        r.pod_layout = Some(layout);
        r
    }

    fn abi_type(
        descriptor: &str,
        direction: NativeAbiDirection,
        js_argument_index: Option<usize>,
        abi_slot_index: usize,
    ) -> NativeAbiTypeRecord {
        let descriptor = perry_api_manifest::NativeAbiType::parse_str(descriptor).unwrap();
        NativeAbiTypeRecord::new(&descriptor, direction, js_argument_index, abi_slot_index)
    }

    #[test]
    fn fails_unsafe_inbounds_without_artifact_output() {
        let mut r = record();
        r.emitted_inbounds = true;
        r.bounds_state = Some(BoundsState::Unknown);
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn fails_unsafe_noalias_without_artifact_output() {
        let mut r = record();
        r.emitted_noalias = true;
        r.alias_state = Some(AliasState::MayAlias);
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn fails_explicit_assume_guard_without_artifact_output() {
        let mut r = record();
        r.bounds_state = Some(BoundsState::Proven {
            proof: BoundsProof::ExplicitAssume,
        });
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn accepts_proven_bounds_and_noalias() {
        let mut r = record();
        r.emitted_inbounds = true;
        r.emitted_noalias = true;
        r.bounds_state = Some(BoundsState::Proven {
            proof: BoundsProof::MinLength,
        });
        r.alias_state = Some(AliasState::NoAliasProven);
        assert!(verify_native_rep_records(&[r]).is_ok());
    }

    #[test]
    fn fails_unchecked_native_unknown_bounds_without_artifact_output() {
        let mut r = record();
        r.access_mode = Some(BufferAccessMode::UncheckedNative);
        r.bounds_state = Some(BoundsState::Unknown);
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn accepts_dynamic_fallback_unknown_bounds() {
        let mut r = record();
        r.access_mode = Some(BufferAccessMode::DynamicFallback);
        r.bounds_state = Some(BoundsState::Unknown);
        r.materialization_reason = Some(crate::native_value::MaterializationReason::UnknownBounds);
        r.fallback_reason = Some(crate::native_value::MaterializationReason::UnknownBounds);
        r.native_value_state = NativeValueState::DynamicFallback;
        assert!(verify_native_rep_records(&[r]).is_ok());
    }

    #[test]
    fn accepts_unchecked_native_proven_and_guarded_bounds() {
        let mut proven = record();
        proven.access_mode = Some(BufferAccessMode::UncheckedNative);
        proven.bounds_state = Some(BoundsState::Proven {
            proof: BoundsProof::MinLength,
        });
        let mut guarded = record();
        guarded.access_mode = Some(BufferAccessMode::UncheckedNative);
        guarded.bounds_state = Some(BoundsState::Guarded {
            guard_id: "loop_guard".to_string(),
        });
        assert!(verify_native_rep_records(&[proven, guarded]).is_ok());
    }

    #[test]
    fn rejects_checked_native_without_real_bounds() {
        let mut r = record();
        r.access_mode = Some(BufferAccessMode::CheckedNative);
        r.bounds_state = Some(BoundsState::Unknown);
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_raw_f64_checked_native_without_consumed_layout_fact() {
        for (expr_kind, consumer) in [
            ("NumericArrayIndexGet", "js_array_numeric_get_f64_unboxed"),
            ("NumericArrayIndexSet", "js_array_numeric_set_f64_unboxed"),
            ("NumericArrayPush", "js_array_numeric_push_f64_unboxed"),
            ("ClassFieldGet", "class_field_get.raw_f64_load"),
            ("ClassFieldSet", "class_field_set.raw_f64_store"),
        ] {
            let mut r = record();
            r.expr_kind = expr_kind.to_string();
            r.consumer = consumer.to_string();
            r.semantic = SemanticKind::JsNumber;
            r.native_rep = NativeRep::F64;
            r.native_rep_name = "f64".to_string();
            r.llvm_ty = DOUBLE;
            r.access_mode = Some(BufferAccessMode::CheckedNative);
            r.bounds_state = Some(BoundsState::Guarded {
                guard_id: "raw_f64_guard".to_string(),
            });

            assert!(
                verify_native_rep_records(&[r.clone()]).is_err(),
                "{consumer} should require a consumed raw_f64_layout fact"
            );

            r.consumed_facts.push(raw_f64_layout_fact("consumed", None));
            assert!(
                verify_native_rep_records(&[r]).is_ok(),
                "{consumer} should verify once the consumed layout fact is present"
            );
        }
    }

    #[test]
    fn rejects_raw_f64_dynamic_fallback_without_rejected_and_invalidated_layout_facts() {
        for (expr_kind, consumer) in [
            ("NumericArrayPush", "js_array_push_f64"),
            (
                "NumericArrayIndexGet",
                "js_typed_feedback_array_index_get_fallback_boxed",
            ),
            (
                "NumericArrayIndexSet",
                "js_typed_feedback_array_index_set_fallback_boxed",
            ),
            ("ClassFieldGet", "js_object_get_field_by_name_f64"),
            ("ClassFieldSet", "js_object_set_field_by_name"),
        ] {
            let mut r = record();
            r.expr_kind = expr_kind.to_string();
            r.consumer = consumer.to_string();
            r.semantic = SemanticKind::JsValue;
            r.native_rep = NativeRep::JsValue;
            r.native_rep_name = "js_value".to_string();
            r.llvm_ty = DOUBLE;
            r.access_mode = Some(BufferAccessMode::DynamicFallback);
            r.materialization_reason = Some(MaterializationReason::RuntimeApi);
            r.fallback_reason = Some(MaterializationReason::RuntimeApi);
            r.native_value_state = NativeValueState::DynamicFallback;

            assert!(
                verify_native_rep_records(&[r.clone()]).is_err(),
                "{consumer} should require rejected and invalidated raw_f64_layout facts"
            );

            r.rejected_facts.push(raw_f64_layout_fact(
                "rejected",
                Some(MaterializationReason::RuntimeApi),
            ));
            assert!(
                verify_native_rep_records(&[r.clone()]).is_err(),
                "{consumer} should still require invalidated raw_f64_layout fact"
            );

            r.rejected_facts.push(raw_f64_layout_fact(
                "invalidated",
                Some(MaterializationReason::RuntimeApi),
            ));
            assert!(
                verify_native_rep_records(&[r]).is_ok(),
                "{consumer} should verify once rejection and invalidation are recorded"
            );
        }
    }

    #[test]
    fn accepts_new_region_local_native_abi_records() {
        let mut f64_record = record();
        f64_record.native_rep = NativeRep::F64;
        f64_record.native_rep_name = "f64".to_string();
        f64_record.llvm_ty = DOUBLE;
        f64_record.llvm_value = "%f".to_string();
        f64_record.native_abi_type = Some(abi_type("f64", NativeAbiDirection::Return, None, 0));

        let mut u32_record = record();
        u32_record.native_rep = NativeRep::U32;
        u32_record.native_rep_name = "u32".to_string();
        u32_record.llvm_ty = I32;
        u32_record.llvm_value = "%u".to_string();
        u32_record.native_abi_type = Some(abi_type("u32", NativeAbiDirection::Param, Some(0), 0));

        let mut u64_record = record();
        u64_record.native_rep = NativeRep::U64;
        u64_record.native_rep_name = "u64".to_string();
        u64_record.llvm_ty = I64;
        u64_record.llvm_value = "%u64".to_string();
        u64_record.native_abi_type = Some(abi_type("u64", NativeAbiDirection::Param, Some(1), 1));

        let mut usize_record = record();
        usize_record.native_rep = NativeRep::USize;
        usize_record.native_rep_name = "usize".to_string();
        usize_record.llvm_ty = I64;
        usize_record.llvm_value = "%usize".to_string();
        usize_record.native_abi_type =
            Some(abi_type("usize", NativeAbiDirection::Param, Some(2), 2));

        let mut f32_record = record();
        f32_record.native_rep = NativeRep::F32;
        f32_record.native_rep_name = "f32".to_string();
        f32_record.llvm_ty = F32;
        f32_record.llvm_value = "%f32".to_string();
        f32_record.native_abi_type = Some(abi_type("f32", NativeAbiDirection::Param, Some(3), 3));

        let mut buffer_len_record = record();
        buffer_len_record.native_rep = NativeRep::BufferLen;
        buffer_len_record.native_rep_name = "buffer_len".to_string();
        buffer_len_record.llvm_ty = I32;
        buffer_len_record.llvm_value = "%len".to_string();
        buffer_len_record.native_abi_type = Some(abi_type(
            "buffer_len",
            NativeAbiDirection::Param,
            Some(4),
            4,
        ));

        let mut handle_record = record();
        handle_record.native_rep = NativeRep::NativeHandle;
        handle_record.native_rep_name = "native_handle".to_string();
        handle_record.llvm_ty = I64;
        handle_record.llvm_value = "%handle".to_string();
        handle_record.native_abi_type = Some(abi_type(
            "handle<MyThing>",
            NativeAbiDirection::Param,
            Some(5),
            5,
        ));

        let mut promise_record = record();
        promise_record.native_rep = NativeRep::PromiseBoundary;
        promise_record.native_rep_name = "promise_boundary".to_string();
        promise_record.llvm_ty = I64;
        promise_record.llvm_value = "%promise".to_string();
        promise_record.native_abi_type = Some(abi_type(
            "promise<f64>",
            NativeAbiDirection::Return,
            None,
            0,
        ));

        assert!(verify_native_rep_records(&[
            f64_record,
            u32_record,
            u64_record,
            usize_record,
            f32_record,
            buffer_len_record,
            handle_record,
            promise_record
        ])
        .is_ok());
    }

    #[test]
    fn rejects_native_abi_descriptor_rep_mismatch() {
        let mut r = record();
        r.native_abi_type = Some(abi_type("f32", NativeAbiDirection::Param, Some(0), 0));
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_native_abi_param_without_js_argument_index() {
        let mut r = record();
        r.native_abi_type = Some(abi_type("i32", NativeAbiDirection::Param, None, 0));
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_native_abi_return_with_js_argument_index() {
        let mut r = record();
        r.native_abi_type = Some(abi_type("i32", NativeAbiDirection::Return, Some(0), 0));
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_buffer_and_len_return_descriptor() {
        let mut r = record();
        r.native_rep = NativeRep::BufferView(BufferViewRep {
            data_ptr: "%ptr".to_string(),
            length: "%len".to_string(),
            elem: crate::native_value::BufferElem::U8,
            element_width_bytes: 1,
            index_unit: crate::native_value::BufferIndexUnit::Byte,
            view_byte_offset: Some(0),
            length_offset_from_data: -8,
            bounds: BoundsState::Unknown,
            alias: AliasState::Unknown,
        });
        r.native_rep_name = "buffer_view".to_string();
        r.llvm_ty = PTR;
        r.native_abi_type = Some(abi_type("buffer+len", NativeAbiDirection::Return, None, 0));
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_handle_abi_missing_native_handle_contract() {
        let mut r = record();
        r.native_rep = NativeRep::NativeHandle;
        r.native_rep_name = "native_handle".to_string();
        r.llvm_ty = I64;
        r.llvm_value = "%handle".to_string();
        r.native_abi_type = Some(abi_type(
            "handle<MyThing>",
            NativeAbiDirection::Param,
            Some(0),
            0,
        ));
        r.native_abi_type.as_mut().unwrap().native_handle = None;

        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_invalid_native_handle_contract_fields() {
        let mut r = record();
        r.native_rep = NativeRep::NativeHandle;
        r.native_rep_name = "native_handle".to_string();
        r.llvm_ty = I64;
        r.llvm_value = "%handle".to_string();
        r.native_abi_type = Some(abi_type(
            "handle<MyThing>",
            NativeAbiDirection::Param,
            Some(0),
            0,
        ));
        let handle = r
            .native_abi_type
            .as_mut()
            .unwrap()
            .native_handle
            .as_mut()
            .unwrap();
        handle.type_id = 0;
        handle.ownership = "leased".to_string();
        handle.thread_affinity = "worker".to_string();
        handle.debug_name.clear();
        handle.has_finalizer = true;
        handle.finalizer_symbol = Some("my_thing_free".to_string());

        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn accepts_verifier_backed_pod_layout() {
        let layout = pod_layout();
        let r = pod_record(layout);
        assert!(verify_native_rep_records(&[r]).is_ok());
    }

    #[test]
    fn rejects_pod_layout_offset_mismatch() {
        let mut layout = pod_layout();
        layout.fields[2].offset = 12;
        let r = pod_record(layout);
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_pod_pointer_mask_without_metadata() {
        let mut layout = pod_layout();
        layout.pointer_mask = vec![1];
        layout.explicit_pointer_metadata = false;
        let r = pod_record(layout);
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_escaping_buffer_view() {
        let mut r = record();
        r.native_rep = NativeRep::BufferView(BufferViewRep {
            data_ptr: "%ptr".to_string(),
            length: "%len".to_string(),
            elem: crate::native_value::BufferElem::U8,
            element_width_bytes: 1,
            index_unit: crate::native_value::BufferIndexUnit::Byte,
            view_byte_offset: Some(0),
            length_offset_from_data: -8,
            bounds: BoundsState::Unknown,
            alias: AliasState::Unknown,
        });
        r.native_rep_name = "buffer_view".to_string();
        r.llvm_ty = crate::types::PTR;
        r.materialization_reason = Some(crate::native_value::MaterializationReason::RuntimeApi);
        r.native_value_state = NativeValueState::Materialized;
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_rep_llvm_type_mismatch() {
        let mut r = record();
        r.native_rep = NativeRep::U32;
        r.native_rep_name = "u32".to_string();
        r.llvm_ty = DOUBLE;
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_dynamic_fallback_without_reason() {
        let mut r = record();
        r.access_mode = Some(BufferAccessMode::DynamicFallback);
        r.native_value_state = NativeValueState::DynamicFallback;
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_invalid_scalar_conversion() {
        let mut r = record();
        r.native_rep = NativeRep::JsValue;
        r.native_rep_name = "js_value".to_string();
        r.llvm_ty = DOUBLE;
        r.native_value_state = NativeValueState::Materialized;
        r.materialization_reason = Some(crate::native_value::MaterializationReason::FunctionAbi);
        r.native_abi_transition = Some(NativeAbiTransitionRecord {
            from_native_rep: "u32".to_string(),
            to_native_rep: "js_value".to_string(),
            op: NativeAbiTransitionOp::SignedIntToFloat,
            reason: crate::native_value::MaterializationReason::FunctionAbi,
            lossy: false,
        });
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_materialized_f32_record() {
        let mut r = record();
        r.native_rep = NativeRep::F32;
        r.native_rep_name = "f32".to_string();
        r.llvm_ty = F32;
        r.materialization_reason = Some(crate::native_value::MaterializationReason::FunctionAbi);
        r.native_value_state = NativeValueState::Materialized;
        assert!(verify_native_rep_records(&[r]).is_err());
    }

    #[test]
    fn rejects_escaping_raw_handle_and_promise() {
        let mut handle = record();
        handle.native_rep = NativeRep::NativeHandle;
        handle.native_rep_name = "native_handle".to_string();
        handle.llvm_ty = I64;
        handle.materialization_reason = Some(crate::native_value::MaterializationReason::ReturnAbi);
        handle.native_value_state = NativeValueState::Materialized;

        let mut promise = record();
        promise.native_rep = NativeRep::PromiseBoundary;
        promise.native_rep_name = "promise_boundary".to_string();
        promise.llvm_ty = I64;
        promise.materialization_reason =
            Some(crate::native_value::MaterializationReason::ReturnAbi);
        promise.native_value_state = NativeValueState::Materialized;

        assert!(verify_native_rep_records(&[handle, promise]).is_err());
    }

    #[test]
    fn accepts_handle_and_promise_boxing_transitions() {
        let mut handle = record();
        handle.native_rep = NativeRep::JsValue;
        handle.native_rep_name = "js_value".to_string();
        handle.llvm_ty = DOUBLE;
        handle.native_value_state = NativeValueState::Materialized;
        handle.materialization_reason = Some(crate::native_value::MaterializationReason::ReturnAbi);
        handle.native_abi_transition = Some(NativeAbiTransitionRecord {
            from_native_rep: "native_handle".to_string(),
            to_native_rep: "js_value".to_string(),
            op: NativeAbiTransitionOp::PointerBox,
            reason: crate::native_value::MaterializationReason::ReturnAbi,
            lossy: false,
        });

        let mut promise = record();
        promise.native_rep = NativeRep::JsValue;
        promise.native_rep_name = "js_value".to_string();
        promise.llvm_ty = DOUBLE;
        promise.native_value_state = NativeValueState::Materialized;
        promise.materialization_reason =
            Some(crate::native_value::MaterializationReason::ReturnAbi);
        promise.native_abi_transition = Some(NativeAbiTransitionRecord {
            from_native_rep: "promise_boundary".to_string(),
            to_native_rep: "js_value".to_string(),
            op: NativeAbiTransitionOp::PromiseBox,
            reason: crate::native_value::MaterializationReason::ReturnAbi,
            lossy: false,
        });

        assert!(verify_native_rep_records(&[handle, promise]).is_ok());
    }
}
