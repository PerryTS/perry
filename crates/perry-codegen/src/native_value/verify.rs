use anyhow::{bail, Result};

use super::artifact::{NativeRepRecord, NativeValueState, ScalarConversionOp};
use super::buffer::{AliasState, BoundsState, BufferAccessMode};
use super::rep::NativeRep;
use crate::types::{DOUBLE, I32, I8, PTR};

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
            record.access_mode.as_ref(),
            Some(BufferAccessMode::DynamicFallback)
        ) && (record.fallback_reason.is_none() || record.materialization_reason.is_none())
        {
            errors.push(format!(
                "{}:{} {} dynamic fallback missing fallback/materialization reason",
                record.function, record.block_label, record.consumer
            ));
        }
        if let Some(conversion) = record.scalar_conversion.as_ref() {
            if !valid_scalar_conversion(
                conversion.from_native_rep.as_str(),
                conversion.to_native_rep.as_str(),
                &conversion.op,
            ) {
                errors.push(format!(
                    "{}:{} {} invalid scalar conversion {} -> {} via {:?}",
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
        NativeRep::I32 | NativeRep::U32 => I32,
        NativeRep::U8 => I8,
        NativeRep::BufferView(_) => PTR,
    })
}

fn valid_scalar_conversion(from: &str, to: &str, op: &ScalarConversionOp) -> bool {
    if to != NativeRep::JsValue.name() {
        return false;
    }
    match op {
        ScalarConversionOp::None => matches!(from, "f64" | "js_value"),
        ScalarConversionOp::SignedIntToFloat => from == "i32",
        ScalarConversionOp::UnsignedIntToFloat => matches!(from, "u8" | "u32"),
    }
}

#[cfg(test)]
mod tests {
    use crate::native_value::{
        verify_native_rep_records, AliasState, BoundsProof, BoundsState, BufferAccessMode,
        BufferViewRep, LoweredValue, NativeRep, NativeRepRecord, NativeValueState,
        ScalarConversionRecord, SemanticKind,
    };
    use super::ScalarConversionOp;
    use crate::types::{DOUBLE, I32};

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
    fn accepts_f64_and_u32_records() {
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

        assert!(verify_native_rep_records(&[f64_record, u32_record]).is_ok());
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
        r.native_value_state = NativeValueState::Materialized;
        r.materialization_reason = Some(crate::native_value::MaterializationReason::FunctionAbi);
        r.scalar_conversion = Some(ScalarConversionRecord {
            from_native_rep: "u32".to_string(),
            to_native_rep: "js_value".to_string(),
            op: ScalarConversionOp::SignedIntToFloat,
            reason: crate::native_value::MaterializationReason::FunctionAbi,
        });
        assert!(verify_native_rep_records(&[r]).is_err());
    }
}
