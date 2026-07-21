//! Issue #1098: extracted `FnCtx::record_lowered_value*` methods.
//!
//! Pure mechanical move out of `expr/mod.rs`. These are inherent methods on
//! `FnCtx`, so no re-export is needed — they attach to the type, not the
//! module path.
use super::*;

use crate::native_value::{
    AliasState, BoundsState, BufferAccessFacts, BufferAccessMode, LoweredValue,
    MaterializationReason, NativeAbiTypeRecord, NativeFactUse, NativeRepRecord, NativeValueState,
    PodLayoutManifest, PodRecordViewManifest, ScalarConversionRecord,
};

impl<'a> FnCtx<'a> {
    pub fn record_lowered_value(
        &mut self,
        expr_kind: impl Into<String>,
        local_id: Option<u32>,
        consumer: impl Into<String>,
        lowered: &LoweredValue,
        bounds_state: Option<BoundsState>,
        alias_state: Option<AliasState>,
        materialization_reason: Option<MaterializationReason>,
        emitted_inbounds: bool,
        emitted_noalias: bool,
        notes: Vec<String>,
    ) {
        self.record_lowered_value_with_access_mode(
            expr_kind,
            local_id,
            consumer,
            lowered,
            bounds_state,
            alias_state,
            None,
            materialization_reason,
            emitted_inbounds,
            emitted_noalias,
            notes,
        );
    }

    pub fn record_lowered_value_with_access_mode(
        &mut self,
        expr_kind: impl Into<String>,
        local_id: Option<u32>,
        consumer: impl Into<String>,
        lowered: &LoweredValue,
        bounds_state: Option<BoundsState>,
        alias_state: Option<AliasState>,
        access_mode: Option<BufferAccessMode>,
        materialization_reason: Option<MaterializationReason>,
        emitted_inbounds: bool,
        emitted_noalias: bool,
        notes: Vec<String>,
    ) {
        self.record_lowered_value_with_access_mode_and_conversion(
            expr_kind,
            local_id,
            consumer,
            lowered,
            bounds_state,
            alias_state,
            access_mode,
            materialization_reason,
            None,
            None,
            emitted_inbounds,
            emitted_noalias,
            notes,
        );
    }

    pub fn record_lowered_value_with_access_mode_and_conversion(
        &mut self,
        expr_kind: impl Into<String>,
        local_id: Option<u32>,
        consumer: impl Into<String>,
        lowered: &LoweredValue,
        bounds_state: Option<BoundsState>,
        alias_state: Option<AliasState>,
        access_mode: Option<BufferAccessMode>,
        materialization_reason: Option<MaterializationReason>,
        scalar_conversion: Option<ScalarConversionRecord>,
        buffer_access: Option<BufferAccessFacts>,
        emitted_inbounds: bool,
        emitted_noalias: bool,
        notes: Vec<String>,
    ) {
        self.record_lowered_value_full(
            expr_kind,
            local_id,
            consumer,
            lowered,
            bounds_state,
            alias_state,
            access_mode,
            materialization_reason,
            scalar_conversion,
            buffer_access,
            Vec::new(),
            Vec::new(),
            None,
            emitted_inbounds,
            emitted_noalias,
            notes,
        );
    }

    pub fn record_lowered_value_with_access_mode_and_facts(
        &mut self,
        expr_kind: impl Into<String>,
        local_id: Option<u32>,
        consumer: impl Into<String>,
        lowered: &LoweredValue,
        bounds_state: Option<BoundsState>,
        alias_state: Option<AliasState>,
        access_mode: Option<BufferAccessMode>,
        materialization_reason: Option<MaterializationReason>,
        scalar_conversion: Option<ScalarConversionRecord>,
        buffer_access: Option<BufferAccessFacts>,
        extra_consumed_facts: Vec<NativeFactUse>,
        extra_rejected_facts: Vec<NativeFactUse>,
        emitted_inbounds: bool,
        emitted_noalias: bool,
        notes: Vec<String>,
    ) {
        self.record_lowered_value_full(
            expr_kind,
            local_id,
            consumer,
            lowered,
            bounds_state,
            alias_state,
            access_mode,
            materialization_reason,
            scalar_conversion,
            buffer_access,
            extra_consumed_facts,
            extra_rejected_facts,
            None,
            emitted_inbounds,
            emitted_noalias,
            notes,
        );
    }

    pub fn record_lowered_value_with_native_abi(
        &mut self,
        expr_kind: impl Into<String>,
        consumer: impl Into<String>,
        lowered: &LoweredValue,
        native_abi_type: NativeAbiTypeRecord,
        notes: Vec<String>,
    ) {
        self.record_lowered_value_full(
            expr_kind,
            None,
            consumer,
            lowered,
            None,
            None,
            None,
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            Some(native_abi_type),
            false,
            false,
            notes,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_lowered_value_with_native_abi_and_pod_layout(
        &mut self,
        expr_kind: impl Into<String>,
        local_id: Option<u32>,
        consumer: impl Into<String>,
        lowered: &LoweredValue,
        native_abi_type: NativeAbiTypeRecord,
        pod_layout: Option<PodLayoutManifest>,
        access_mode: Option<BufferAccessMode>,
        materialization_reason: Option<MaterializationReason>,
        notes: Vec<String>,
    ) {
        self.record_lowered_value_full(
            expr_kind,
            local_id,
            consumer,
            lowered,
            None,
            None,
            access_mode,
            materialization_reason,
            None,
            None,
            Vec::new(),
            Vec::new(),
            Some(native_abi_type),
            false,
            false,
            notes,
        );
        if let Some(layout) = pod_layout {
            if let Some(record) = self.native_rep_records.last_mut() {
                record.pod_layout = Some(layout);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_lowered_value_with_native_abi_and_pod_view(
        &mut self,
        expr_kind: impl Into<String>,
        local_id: Option<u32>,
        consumer: impl Into<String>,
        lowered: &LoweredValue,
        native_abi_type: NativeAbiTypeRecord,
        pod_layout: Option<PodLayoutManifest>,
        pod_record_view: PodRecordViewManifest,
        access_mode: Option<BufferAccessMode>,
        materialization_reason: Option<MaterializationReason>,
        notes: Vec<String>,
    ) {
        self.record_lowered_value_full(
            expr_kind,
            local_id,
            consumer,
            lowered,
            None,
            None,
            access_mode,
            materialization_reason,
            None,
            None,
            Vec::new(),
            Vec::new(),
            Some(native_abi_type),
            false,
            false,
            notes,
        );
        if let Some(record) = self.native_rep_records.last_mut() {
            record.pod_layout = pod_layout;
            record.pod_record_view = Some(pod_record_view);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record_lowered_value_full(
        &mut self,
        expr_kind: impl Into<String>,
        local_id: Option<u32>,
        consumer: impl Into<String>,
        lowered: &LoweredValue,
        bounds_state: Option<BoundsState>,
        alias_state: Option<AliasState>,
        access_mode: Option<BufferAccessMode>,
        materialization_reason: Option<MaterializationReason>,
        scalar_conversion: Option<ScalarConversionRecord>,
        buffer_access: Option<BufferAccessFacts>,
        extra_consumed_facts: Vec<NativeFactUse>,
        extra_rejected_facts: Vec<NativeFactUse>,
        native_abi_type: Option<NativeAbiTypeRecord>,
        emitted_inbounds: bool,
        emitted_noalias: bool,
        notes: Vec<String>,
    ) {
        let block_label = self.current_block_label();
        let (mut consumed_facts, mut rejected_facts) =
            super::native_record::native_fact_uses_for_record(
                local_id,
                lowered,
                bounds_state.as_ref(),
                alias_state.as_ref(),
                access_mode.as_ref(),
                materialization_reason.as_ref(),
            );
        consumed_facts.extend(extra_consumed_facts);
        rejected_facts.extend(extra_rejected_facts);
        let fallback_reason = if matches!(
            access_mode.as_ref(),
            Some(BufferAccessMode::DynamicFallback)
        ) {
            materialization_reason.clone()
        } else {
            None
        };
        let native_value_state = if matches!(
            access_mode.as_ref(),
            Some(BufferAccessMode::DynamicFallback)
        ) {
            NativeValueState::DynamicFallback
        } else if materialization_reason.is_some() {
            NativeValueState::Materialized
        } else {
            NativeValueState::RegionLocal
        };
        self.native_rep_records.push(NativeRepRecord {
            function: self.func.name.clone(),
            block_label: block_label.clone(),
            region_id: self.active_region_id.clone(),
            source_function: self.source_function.clone(),
            lowering_block: block_label,
            local_id,
            expr_kind: expr_kind.into(),
            source_key: None,
            semantic: lowered.semantic.clone(),
            native_rep: lowered.rep.clone(),
            native_rep_name: lowered.rep.name().to_string(),
            llvm_ty: lowered.llvm_ty,
            llvm_value: lowered.value.clone(),
            consumer: consumer.into(),
            bounds_state,
            alias_state,
            access_mode,
            buffer_access,
            native_owned_view: None,
            materialization_reason,
            fallback_reason,
            native_value_state,
            native_abi_transition: scalar_conversion.clone(),
            scalar_conversion,
            native_abi_type,
            pod_layout: None,
            pod_record_view: None,
            consumed_facts,
            rejected_facts,
            emitted_inbounds,
            emitted_noalias,
            notes,
        });
    }
}
