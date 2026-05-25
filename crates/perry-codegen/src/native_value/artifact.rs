use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::types::LlvmType;

use super::buffer::{AliasState, BoundsState, BufferAccessMode};
use super::materialize::MaterializationReason;
use super::rep::{NativeRep, SemanticKind};

static NATIVE_REP_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativeFactUse {
    pub fact_id: String,
    pub kind: String,
    pub local_id: Option<u32>,
    pub state: String,
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
    SignedIntToFloat,
    UnsignedIntToFloat,
    FloatExtend,
    PointerBox,
    PromiseBox,
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
    pub materialization_reason: Option<MaterializationReason>,
    pub fallback_reason: Option<MaterializationReason>,
    pub native_value_state: NativeValueState,
    pub native_abi_transition: Option<NativeAbiTransitionRecord>,
    pub scalar_conversion: Option<ScalarConversionRecord>,
    pub consumed_facts: Vec<NativeFactUse>,
    pub rejected_facts: Vec<NativeFactUse>,
    pub emitted_inbounds: bool,
    pub emitted_noalias: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct NativeRepArtifact<'a> {
    schema_version: u32,
    module: &'a str,
    records: &'a [NativeRepRecord],
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
        for record in records {
            *native_rep_counts
                .entry(record.native_rep_name.clone())
                .or_insert(0) += 1;
            if record.materialization_reason.is_some() {
                materialization_count += 1;
            }
            if let Some(transition) = record.native_abi_transition.as_ref() {
                native_abi_transition_count += 1;
                let op_name = match transition.op {
                    NativeAbiTransitionOp::None => "none",
                    NativeAbiTransitionOp::SignedIntToFloat => "signed_int_to_float",
                    NativeAbiTransitionOp::UnsignedIntToFloat => "unsigned_int_to_float",
                    NativeAbiTransitionOp::FloatExtend => "float_extend",
                    NativeAbiTransitionOp::PointerBox => "pointer_box",
                    NativeAbiTransitionOp::PromiseBox => "promise_box",
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
        }
    }
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
        schema_version: 5,
        module,
        records,
        summary: NativeRepSummary::from_records(records),
    };
    let text = serde_json::to_string_pretty(&artifact)?;
    std::fs::write(&path, format!("{}\n", text))
        .with_context(|| format!("failed to write native reps at {}", path.display()))?;
    eprintln!("[perry-codegen] kept native reps: {}", path.display());
    Ok(Some(path))
}
