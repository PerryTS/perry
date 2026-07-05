use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::types::{LlvmType, DOUBLE};

use super::buffer::{
    AliasState, BoundsState, BufferAccessFacts, BufferAccessMode, NativeOwnedViewFact,
};
use super::materialize::MaterializationReason;
use super::rep::{NativeRep, SemanticKind};

static NATIVE_REP_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativeFactUse {
    pub fact_id: String,
    pub kind: String,
    pub local_id: Option<u32>,
    pub state: String,
    pub detail: String,
    pub reason: Option<MaterializationReason>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeValueState {
    RegionLocal,
    Materialized,
    DynamicFallback,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeAbiTransitionOp {
    None,
    JsValueToBits,
    BitsToJsValue,
    SignedIntToFloat,
    UnsignedIntToFloat,
    FloatExtend,
    PointerBox,
    NativeHandleBox,
    PromiseBox,
    BoolToJsValue,
    #[serde(rename = "bigint_box")]
    BigIntBox,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct NativeAbiTransitionRecord {
    pub from_native_rep: String,
    pub to_native_rep: String,
    pub op: NativeAbiTransitionOp,
    pub reason: MaterializationReason,
    pub lossy: bool,
}

pub(crate) type ScalarConversionRecord = NativeAbiTransitionRecord;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeAbiDirection {
    Param,
    Return,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct NativeAbiTypeRecord {
    pub canonical_kind: String,
    pub display: String,
    pub direction: NativeAbiDirection,
    pub js_argument_index: Option<usize>,
    pub abi_slot_index: usize,
    pub abi_slot_count: usize,
    pub runtime_guard: Option<NativeRuntimeGuardRecord>,
    pub handle_type: Option<String>,
    pub native_handle: Option<NativeHandleContractRecord>,
    pub promise_result: Option<String>,
    pub promise_completion: Option<String>,
    pub promise_thread: Option<String>,
    pub pod_name: Option<String>,
    pub pod_fields: Vec<NativePodFieldContractRecord>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct NativeRuntimeGuardRecord {
    pub helper: String,
    pub requirement: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct NativeHandleContractRecord {
    pub type_name: Option<String>,
    pub type_id: u64,
    pub ownership: String,
    pub nullable: bool,
    pub thread_affinity: String,
    pub debug_name: String,
    pub finalizer_symbol: Option<String>,
    pub has_finalizer: bool,
    pub direction: NativeAbiDirection,
    pub js_argument_index: Option<usize>,
    pub abi_slot_index: usize,
    pub abi_slot_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct NativePodFieldContractRecord {
    pub name: String,
    pub path: Vec<String>,
    pub ty: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct PodRecordViewManifest {
    pub layout_id: String,
    pub stride: u32,
    pub alignment: u32,
    pub count_source: String,
    pub pointer_free_backing: bool,
    pub endian: String,
    pub packing: String,
}

fn push_pod_field_contracts(
    out: &mut Vec<NativePodFieldContractRecord>,
    prefix: &mut Vec<String>,
    fields: &[perry_api_manifest::NativePodFieldAbi],
) {
    for field in fields {
        prefix.push(field.name.clone());
        match &field.ty {
            perry_api_manifest::NativeAbiType::Pod(pod) => {
                push_pod_field_contracts(out, prefix, &pod.fields);
            }
            ty => out.push(NativePodFieldContractRecord {
                name: prefix.join("."),
                path: prefix.clone(),
                ty: ty.canonical_kind().to_string(),
            }),
        }
        prefix.pop();
    }
}

impl NativeAbiTypeRecord {
    pub(crate) fn new(
        descriptor: &perry_api_manifest::NativeAbiType,
        direction: NativeAbiDirection,
        js_argument_index: Option<usize>,
        abi_slot_index: usize,
    ) -> Self {
        let native_handle = descriptor
            .handle_abi()
            .map(|handle| NativeHandleContractRecord {
                type_name: handle.type_name.clone(),
                type_id: handle.type_id(),
                ownership: handle.ownership.as_str().to_string(),
                nullable: handle.nullable,
                thread_affinity: handle.thread.as_str().to_string(),
                debug_name: handle.debug_name.clone(),
                finalizer_symbol: handle.finalizer.clone(),
                has_finalizer: handle.finalizer.is_some(),
                direction: direction.clone(),
                js_argument_index,
                abi_slot_index,
                abi_slot_count: descriptor.abi_slot_count(),
            });
        Self {
            canonical_kind: descriptor.canonical_kind().to_string(),
            display: descriptor.to_string(),
            direction,
            js_argument_index,
            abi_slot_index,
            abi_slot_count: descriptor.abi_slot_count(),
            runtime_guard: None,
            handle_type: descriptor.handle_type().map(str::to_string),
            native_handle,
            promise_result: descriptor.promise_result().map(ToString::to_string),
            promise_completion: descriptor
                .promise_completion()
                .map(|completion| completion.as_str().to_string()),
            promise_thread: descriptor
                .promise_thread()
                .map(|thread| thread.as_str().to_string()),
            pod_name: descriptor
                .pod_abi()
                .and_then(|pod| pod.name.as_ref().map(ToString::to_string)),
            pod_fields: descriptor
                .pod_abi()
                .map(|pod| {
                    let mut fields = Vec::new();
                    let mut prefix = Vec::new();
                    push_pod_field_contracts(&mut fields, &mut prefix, &pod.fields);
                    fields
                })
                .unwrap_or_default(),
        }
    }

    pub(crate) fn with_runtime_guard(
        mut self,
        helper: impl Into<String>,
        requirement: impl Into<String>,
    ) -> Self {
        self.runtime_guard = Some(NativeRuntimeGuardRecord {
            helper: helper.into(),
            requirement: requirement.into(),
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct PodLayoutPadding {
    pub offset: u32,
    pub size: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct PodLayoutField {
    pub name: String,
    pub path: Vec<String>,
    pub native_rep: NativeRep,
    pub native_rep_name: String,
    pub offset: u32,
    pub size: u32,
    pub alignment: u32,
    pub padding_before: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct PodLayoutManifest {
    pub layout_id: String,
    pub size: u32,
    pub alignment: u32,
    pub endian: String,
    pub packing: String,
    pub fields: Vec<PodLayoutField>,
    pub padding: Vec<PodLayoutPadding>,
    pub tail_padding: u32,
    pub pointer_mask: Vec<u64>,
    pub materialization_hazards: Vec<String>,
    pub explicit_pointer_metadata: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativeRepRecord {
    pub function: String,
    pub block_label: String,
    pub region_id: Option<String>,
    pub source_function: String,
    pub lowering_block: String,
    pub local_id: Option<u32>,
    pub expr_kind: String,
    pub source_key: Option<String>,
    pub semantic: SemanticKind,
    pub native_rep: NativeRep,
    pub native_rep_name: String,
    pub llvm_ty: LlvmType,
    pub llvm_value: String,
    pub consumer: String,
    pub bounds_state: Option<BoundsState>,
    pub alias_state: Option<AliasState>,
    pub access_mode: Option<BufferAccessMode>,
    pub buffer_access: Option<BufferAccessFacts>,
    pub native_owned_view: Option<NativeOwnedViewFact>,
    pub materialization_reason: Option<MaterializationReason>,
    pub fallback_reason: Option<MaterializationReason>,
    pub native_value_state: NativeValueState,
    pub native_abi_transition: Option<NativeAbiTransitionRecord>,
    pub scalar_conversion: Option<ScalarConversionRecord>,
    pub native_abi_type: Option<NativeAbiTypeRecord>,
    pub pod_layout: Option<PodLayoutManifest>,
    pub pod_record_view: Option<PodRecordViewManifest>,
    pub consumed_facts: Vec<NativeFactUse>,
    pub rejected_facts: Vec<NativeFactUse>,
    pub emitted_inbounds: bool,
    pub emitted_noalias: bool,
    pub notes: Vec<String>,
}

pub(crate) fn typed_clone_rejection_record(
    source_function: impl Into<String>,
    consumer: impl Into<String>,
    reason: impl Into<String>,
    mut notes: Vec<String>,
) -> NativeRepRecord {
    let source_function = source_function.into();
    let consumer = consumer.into();
    let reason = reason.into();
    notes.insert(0, format!("typed_clone_rejected={reason}"));
    NativeRepRecord {
        function: source_function.clone(),
        block_label: "typed_clone_decision".to_string(),
        region_id: None,
        source_function,
        lowering_block: "typed_clone_decision".to_string(),
        local_id: None,
        expr_kind: "TypedCloneDecision".to_string(),
        source_key: None,
        semantic: SemanticKind::JsValue,
        native_rep: NativeRep::JsValue,
        native_rep_name: NativeRep::JsValue.name().to_string(),
        llvm_ty: DOUBLE,
        llvm_value: "0.0".to_string(),
        consumer,
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
        pod_record_view: None,
        consumed_facts: Vec::new(),
        rejected_facts: Vec::new(),
        emitted_inbounds: false,
        emitted_noalias: false,
        notes,
    }
}

#[derive(Debug, Serialize)]
struct NativeRepArtifact<'a> {
    schema_version: u32,
    module: &'a str,
    records: &'a [NativeRepRecord],
    pod_layouts: Vec<PodLayoutManifest>,
    summary: NativeRepSummary,
}

#[derive(Debug, Serialize)]
struct NativeRepSummary {
    record_count: usize,
    native_rep_counts: HashMap<String, usize>,
    materialization_count: usize,
    native_abi_transition_count: usize,
    native_abi_transition_op_counts: HashMap<String, usize>,
    native_value_state_counts: HashMap<String, usize>,
    unsafe_inbounds_claims: usize,
    unsafe_noalias_claims: usize,
    unsafe_unchecked_unknown_bounds_accesses: usize,
    consumed_fact_count: usize,
    rejected_fact_count: usize,
    consumed_fact_kind_counts: BTreeMap<String, usize>,
    rejected_fact_kind_counts: BTreeMap<String, usize>,
    typed_path_decision_counts: BTreeMap<String, usize>,
    raw_f64_layout_fact_counts: BTreeMap<String, usize>,
    js_value_bits_count: usize,
    write_barrier_elided_count: usize,
    native_owned_view_count: usize,
    pod_layout_count: usize,
    pod_record_count: usize,
    pod_record_view_count: usize,
    pod_materialization_count: usize,
}

impl NativeRepSummary {
    fn from_records(records: &[NativeRepRecord]) -> Self {
        let mut native_rep_counts = HashMap::new();
        let mut native_value_state_counts = HashMap::new();
        let mut native_abi_transition_op_counts = HashMap::new();
        let mut materialization_count = 0;
        let mut native_abi_transition_count = 0;
        let mut unsafe_inbounds_claims = 0;
        let mut unsafe_noalias_claims = 0;
        let mut unsafe_unchecked_unknown_bounds_accesses = 0;
        let mut consumed_fact_count = 0;
        let mut rejected_fact_count = 0;
        let mut consumed_fact_kind_counts = BTreeMap::new();
        let mut rejected_fact_kind_counts = BTreeMap::new();
        let mut typed_path_decision_counts = BTreeMap::new();
        let mut raw_f64_layout_fact_counts = BTreeMap::from([
            ("consumed".to_string(), 0),
            ("rejected".to_string(), 0),
            ("invalidated".to_string(), 0),
        ]);
        let mut js_value_bits_count = 0;
        let mut write_barrier_elided_count = 0;
        let mut native_owned_view_count = 0;
        let mut pod_layout_count = 0;
        let mut pod_record_count = 0;
        let mut pod_record_view_count = 0;
        let mut pod_materialization_count = 0;
        for record in records {
            *native_rep_counts
                .entry(record.native_rep_name.clone())
                .or_insert(0) += 1;
            if matches!(record.native_rep, NativeRep::JsValueBits) {
                js_value_bits_count += 1;
            }
            if record.expr_kind == "WriteBarrierElided" {
                write_barrier_elided_count += 1;
            }
            if record.materialization_reason.is_some() {
                materialization_count += 1;
            }
            if record.pod_layout.is_some() {
                pod_layout_count += 1;
            }
            if record.native_owned_view.is_some() {
                native_owned_view_count += 1;
            }
            if matches!(record.native_rep, NativeRep::PodRecord { .. }) {
                pod_record_count += 1;
            }
            if matches!(record.native_rep, NativeRep::PodRecordView { .. })
                || record.pod_record_view.is_some()
            {
                pod_record_view_count += 1;
            }
            if matches!(
                record.materialization_reason,
                Some(MaterializationReason::PodMaterialization)
            ) {
                pod_materialization_count += 1;
            }
            if let Some(transition) = record.native_abi_transition.as_ref() {
                native_abi_transition_count += 1;
                let op_name = match transition.op {
                    NativeAbiTransitionOp::None => "none",
                    NativeAbiTransitionOp::JsValueToBits => "js_value_to_bits",
                    NativeAbiTransitionOp::BitsToJsValue => "bits_to_js_value",
                    NativeAbiTransitionOp::SignedIntToFloat => "signed_int_to_float",
                    NativeAbiTransitionOp::UnsignedIntToFloat => "unsigned_int_to_float",
                    NativeAbiTransitionOp::FloatExtend => "float_extend",
                    NativeAbiTransitionOp::PointerBox => "pointer_box",
                    NativeAbiTransitionOp::NativeHandleBox => "native_handle_box",
                    NativeAbiTransitionOp::PromiseBox => "promise_box",
                    NativeAbiTransitionOp::BoolToJsValue => "bool_to_js_value",
                    NativeAbiTransitionOp::BigIntBox => "bigint_box",
                };
                *native_abi_transition_op_counts
                    .entry(op_name.to_string())
                    .or_insert(0) += 1;
            }
            let state_name = match record.native_value_state {
                NativeValueState::RegionLocal => "region_local",
                NativeValueState::Materialized => "materialized",
                NativeValueState::DynamicFallback => "dynamic_fallback",
            };
            *native_value_state_counts
                .entry(state_name.to_string())
                .or_insert(0) += 1;
            if record.emitted_inbounds
                && !matches!(
                    record.bounds_state,
                    Some(BoundsState::Proven { .. } | BoundsState::Guarded { .. })
                )
            {
                unsafe_inbounds_claims += 1;
            }
            if record.emitted_noalias
                && !matches!(
                    record.alias_state,
                    Some(AliasState::NoAliasProven | AliasState::NoAliasGuarded { .. })
                )
            {
                unsafe_noalias_claims += 1;
            }
            if matches!(
                record.access_mode.as_ref(),
                Some(BufferAccessMode::UncheckedNative)
            ) && !matches!(
                record.bounds_state,
                Some(BoundsState::Proven { .. } | BoundsState::Guarded { .. })
            ) {
                unsafe_unchecked_unknown_bounds_accesses += 1;
            }
            consumed_fact_count += record.consumed_facts.len();
            rejected_fact_count += record.rejected_facts.len();
            for fact in &record.consumed_facts {
                *consumed_fact_kind_counts
                    .entry(fact.kind.clone())
                    .or_insert(0) += 1;
            }
            for fact in &record.rejected_facts {
                *rejected_fact_kind_counts
                    .entry(fact.kind.clone())
                    .or_insert(0) += 1;
            }
            if record
                .notes
                .iter()
                .any(|note| note.contains("typed_clone="))
                || record
                    .consumed_facts
                    .iter()
                    .any(|fact| fact.kind.starts_with("typed_") || fact.kind == "type_fact")
            {
                *typed_path_decision_counts
                    .entry("selected".to_string())
                    .or_insert(0) += 1;
            }
            if record.notes.iter().any(|note| {
                note.contains("generic_wrapper=")
                    || note.contains("generic_method=")
                    || note.contains("generic_closure=")
            }) || record.fallback_reason.is_some()
            {
                *typed_path_decision_counts
                    .entry("fallback".to_string())
                    .or_insert(0) += 1;
            }
            if record
                .notes
                .iter()
                .any(|note| note.contains("typed_clone_rejected="))
                || record
                    .rejected_facts
                    .iter()
                    .any(|fact| fact.kind.starts_with("typed_") || fact.kind == "type_fact")
            {
                *typed_path_decision_counts
                    .entry("rejected".to_string())
                    .or_insert(0) += 1;
            }
            for fact in record
                .consumed_facts
                .iter()
                .chain(record.rejected_facts.iter())
            {
                if fact.kind == "raw_f64_layout" {
                    *raw_f64_layout_fact_counts
                        .entry(fact.state.clone())
                        .or_insert(0) += 1;
                }
            }
        }
        Self {
            record_count: records.len(),
            native_rep_counts,
            materialization_count,
            native_abi_transition_count,
            native_abi_transition_op_counts,
            native_value_state_counts,
            unsafe_inbounds_claims,
            unsafe_noalias_claims,
            unsafe_unchecked_unknown_bounds_accesses,
            consumed_fact_count,
            rejected_fact_count,
            consumed_fact_kind_counts,
            rejected_fact_kind_counts,
            typed_path_decision_counts,
            raw_f64_layout_fact_counts,
            js_value_bits_count,
            write_barrier_elided_count,
            native_owned_view_count,
            pod_layout_count,
            pod_record_count,
            pod_record_view_count,
            pod_materialization_count,
        }
    }
}

fn collect_pod_layouts(records: &[NativeRepRecord]) -> Vec<PodLayoutManifest> {
    let mut seen = std::collections::HashSet::new();
    let mut layouts = Vec::new();
    for record in records {
        if let Some(layout) = record.pod_layout.as_ref() {
            if seen.insert(layout.layout_id.clone()) {
                layouts.push(layout.clone());
            }
        }
    }
    layouts
}

pub(crate) fn write_native_rep_artifact_if_enabled(
    module: &str,
    records: &[NativeRepRecord],
) -> Result<Option<PathBuf>> {
    if std::env::var_os("PERRY_LLVM_KEEP_IR").is_none()
        && std::env::var_os("PERRY_NATIVE_REPS").is_none()
    {
        return Ok(None);
    }

    let pid = std::process::id();
    let counter = NATIVE_REP_NONCE.fetch_add(1, Ordering::Relaxed);
    let wall_nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let artifact_dir = match std::env::var_os("PERRY_NATIVE_REPS_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).with_context(|| {
                format!("failed to create native reps directory {}", dir.display())
            })?;
            dir
        }
        None => std::env::temp_dir(),
    };
    let path = artifact_dir.join(format!(
        "perry_native_reps_{}_{}_{}.json",
        pid, wall_nonce, counter
    ));
    let artifact = NativeRepArtifact {
        schema_version: 15,
        module,
        records,
        pod_layouts: collect_pod_layouts(records),
        summary: NativeRepSummary::from_records(records),
    };
    let text = serde_json::to_string_pretty(&artifact)?;
    std::fs::write(&path, format!("{}\n", text))
        .with_context(|| format!("failed to write native reps at {}", path.display()))?;
    eprintln!("[perry-codegen] kept native reps: {}", path.display());
    Ok(Some(path))
}
