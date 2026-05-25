use anyhow::{bail, Result};

#[cfg(test)]
use super::artifact::NativeAbiTransitionRecord;
use super::artifact::{NativeAbiTransitionOp, NativeRepRecord, NativeValueState};
use super::buffer::{AliasState, BoundsState, BufferAccessMode};
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
    }
    if !errors.is_empty() {
        bail!(
            "native representation verifier failed: {}",
            errors.join("; ")
        );
    }
    Ok(())
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
    })
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
        NativeAbiTransitionOp::PromiseBox => from == "promise_boundary" && !lossy,
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeAbiTransitionOp, NativeAbiTransitionRecord};
    use crate::native_value::{
        verify_native_rep_records, AliasState, BoundsProof, BoundsState, BufferAccessMode,
        BufferViewRep, LoweredValue, NativeRep, NativeRepRecord, NativeValueState, SemanticKind,
    };
    use crate::types::{DOUBLE, F32, I32, I64};

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
            materialization_reason: None,
            fallback_reason: None,
            native_value_state: NativeValueState::RegionLocal,
            native_abi_transition: None,
            scalar_conversion: None,
            consumed_facts: Vec::new(),
            rejected_facts: Vec::new(),
            emitted_inbounds: false,
            emitted_noalias: false,
            notes: Vec::new(),
        }
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
    fn accepts_new_region_local_native_abi_records() {
        let mut f64_record = record();
        f64_record.native_rep = NativeRep::F64;
        f64_record.native_rep_name = "f64".to_string();
        f64_record.llvm_ty = DOUBLE;
        f64_record.llvm_value = "%f".to_string();

        let mut u32_record = record();
        u32_record.native_rep = NativeRep::U32;
        u32_record.native_rep_name = "u32".to_string();
        u32_record.llvm_ty = I32;
        u32_record.llvm_value = "%u".to_string();

        let mut u64_record = record();
        u64_record.native_rep = NativeRep::U64;
        u64_record.native_rep_name = "u64".to_string();
        u64_record.llvm_ty = I64;
        u64_record.llvm_value = "%u64".to_string();

        let mut usize_record = record();
        usize_record.native_rep = NativeRep::USize;
        usize_record.native_rep_name = "usize".to_string();
        usize_record.llvm_ty = I64;
        usize_record.llvm_value = "%usize".to_string();

        let mut f32_record = record();
        f32_record.native_rep = NativeRep::F32;
        f32_record.native_rep_name = "f32".to_string();
        f32_record.llvm_ty = F32;
        f32_record.llvm_value = "%f32".to_string();

        let mut buffer_len_record = record();
        buffer_len_record.native_rep = NativeRep::BufferLen;
        buffer_len_record.native_rep_name = "buffer_len".to_string();
        buffer_len_record.llvm_ty = I32;
        buffer_len_record.llvm_value = "%len".to_string();

        let mut handle_record = record();
        handle_record.native_rep = NativeRep::NativeHandle;
        handle_record.native_rep_name = "native_handle".to_string();
        handle_record.llvm_ty = I64;
        handle_record.llvm_value = "%handle".to_string();

        let mut promise_record = record();
        promise_record.native_rep = NativeRep::PromiseBoundary;
        promise_record.native_rep_name = "promise_boundary".to_string();
        promise_record.llvm_ty = I64;
        promise_record.llvm_value = "%promise".to_string();

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
    fn rejects_escaping_buffer_view() {
        let mut r = record();
        r.native_rep = NativeRep::BufferView(BufferViewRep {
            data_ptr: "%ptr".to_string(),
            length: "%len".to_string(),
            elem: crate::native_value::BufferElem::U8,
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
