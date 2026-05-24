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
    unsafe_inbounds_claims: usize,
    unsafe_noalias_claims: usize,
    unsafe_unchecked_unknown_bounds_accesses: usize,
}

impl NativeRepSummary {
    fn from_records(records: &[NativeRepRecord]) -> Self {
        let mut native_rep_counts = HashMap::new();
        let mut materialization_count = 0;
        let mut unsafe_inbounds_claims = 0;
        let mut unsafe_noalias_claims = 0;
        let mut unsafe_unchecked_unknown_bounds_accesses = 0;
        for record in records {
            *native_rep_counts
                .entry(record.native_rep_name.clone())
                .or_insert(0) += 1;
            if record.materialization_reason.is_some() {
                materialization_count += 1;
            }
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
        }
        Self {
            record_count: records.len(),
            native_rep_counts,
            materialization_count,
            unsafe_inbounds_claims,
            unsafe_noalias_claims,
            unsafe_unchecked_unknown_bounds_accesses,
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
    let path = std::env::temp_dir().join(format!(
        "perry_native_reps_{}_{}_{}.json",
        pid, wall_nonce, counter
    ));
    let artifact = NativeRepArtifact {
        schema_version: 3,
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
