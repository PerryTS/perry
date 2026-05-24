use anyhow::{bail, Result};

use super::artifact::NativeRepRecord;
use super::buffer::{AliasState, BoundsState, BufferAccessMode};

pub(crate) fn verify_native_rep_records(records: &[NativeRepRecord]) -> Result<()> {
    let mut errors = Vec::new();
    for record in records {
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

#[cfg(test)]
mod tests {
    use crate::native_value::{
        verify_native_rep_records, AliasState, BoundsProof, BoundsState, BufferAccessMode,
        LoweredValue, NativeRep, NativeRepRecord, SemanticKind,
    };
    use crate::types::I32;

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
}
