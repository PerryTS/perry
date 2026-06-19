use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::OutputFormat;

const REPORT_VERSION: u32 = 1;
const MAX_EVIDENCE_ROWS: usize = 20;
const NOT_RECORDED: &str = "not_recorded";
const ALL_TYPED_CLONE_REJECTIONS_ENV: &str = "PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS";

pub(super) struct ExplainLoweringRun {
    artifact_dir: PathBuf,
    report_path: PathBuf,
    old_native_reps: Option<std::ffi::OsString>,
    old_native_reps_dir: Option<std::ffi::OsString>,
    old_all_typed_clone_rejections: Option<std::ffi::OsString>,
}

impl ExplainLoweringRun {
    pub(super) fn prepare(cache_root: &Path) -> Result<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let artifact_dir = cache_root
            .join(".perry-trace")
            .join("lowering")
            .join(format!("{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&artifact_dir).with_context(|| {
            format!(
                "failed to create explain-lowering directory {}",
                artifact_dir.display()
            )
        })?;

        let old_native_reps = std::env::var_os("PERRY_NATIVE_REPS");
        let old_native_reps_dir = std::env::var_os("PERRY_NATIVE_REPS_DIR");
        let old_all_typed_clone_rejections = std::env::var_os(ALL_TYPED_CLONE_REJECTIONS_ENV);
        std::env::set_var("PERRY_NATIVE_REPS", "1");
        std::env::set_var("PERRY_NATIVE_REPS_DIR", &artifact_dir);
        std::env::set_var(ALL_TYPED_CLONE_REJECTIONS_ENV, "1");

        Ok(Self {
            report_path: artifact_dir.join("explain-lowering.json"),
            artifact_dir,
            old_native_reps,
            old_native_reps_dir,
            old_all_typed_clone_rejections,
        })
    }

    pub(super) fn emit(&self, format: OutputFormat) -> Result<PathBuf> {
        let mut report = build_report_from_dir(&self.artifact_dir)?;
        report.report_path = self.report_path.display().to_string();
        let text = serde_json::to_string_pretty(&report)?;
        std::fs::write(&self.report_path, format!("{text}\n")).with_context(|| {
            format!(
                "failed to write explain-lowering report {}",
                self.report_path.display()
            )
        })?;

        match format {
            OutputFormat::Text => print_text_report(&report),
            OutputFormat::Json => {
                eprintln!("[explain-lowering] report: {}", self.report_path.display());
            }
        }

        Ok(self.report_path.clone())
    }
}

impl Drop for ExplainLoweringRun {
    fn drop(&mut self) {
        match &self.old_native_reps {
            Some(value) => std::env::set_var("PERRY_NATIVE_REPS", value),
            None => std::env::remove_var("PERRY_NATIVE_REPS"),
        }
        match &self.old_native_reps_dir {
            Some(value) => std::env::set_var("PERRY_NATIVE_REPS_DIR", value),
            None => std::env::remove_var("PERRY_NATIVE_REPS_DIR"),
        }
        match &self.old_all_typed_clone_rejections {
            Some(value) => std::env::set_var(ALL_TYPED_CLONE_REJECTIONS_ENV, value),
            None => std::env::remove_var(ALL_TYPED_CLONE_REJECTIONS_ENV),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ExplainLoweringReport {
    pub version: u32,
    pub artifact_dir: String,
    pub report_path: String,
    pub artifact_count: usize,
    pub modules: Vec<String>,
    pub summary: LoweringSummary,
    pub evidence: LoweringEvidence,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct LoweringSummary {
    pub record_count: u64,
    pub boxes_inserted: u64,
    pub unboxes_or_coercions: u64,
    pub runtime_property_gets: u64,
    pub direct_field_loads: u64,
    pub bounds_eliminations: u64,
    pub barrier_eliminations: u64,
    pub barrier_emissions: u64,
    pub scalar_replacements: u64,
    pub scalar_replacement_fallbacks: u64,
    pub scalar_replacement_rejections: u64,
    pub typed_path_selections: u64,
    pub typed_path_fallbacks: u64,
    pub typed_path_rejections: u64,
    pub typed_clone_selections: u64,
    pub typed_clone_fallback_decisions: u64,
    pub generic_fallback_emissions: u64,
    pub dynamic_fallbacks: u64,
    pub js_value_bits_records: u64,
    pub native_owned_views: u64,
    pub pod_layouts: u64,
    pub pod_records: u64,
    pub pod_record_views: u64,
    pub pod_materializations: u64,
    pub collection_helper_selections: u64,
    pub collection_helper_fallback_decisions: u64,
    pub collection_typed_value_selections: u64,
    pub collection_typed_value_fallback_decisions: u64,
    pub native_rep_counts: BTreeMap<String, u64>,
    pub native_value_state_counts: BTreeMap<String, u64>,
    pub access_mode_counts: BTreeMap<String, u64>,
    pub materialization_reason_counts: BTreeMap<String, u64>,
    pub fallback_reason_counts: BTreeMap<String, u64>,
    pub scalar_conversion_counts: BTreeMap<String, u64>,
    pub typed_clone_decision_counts: BTreeMap<String, u64>,
    pub typed_clone_selection_reason_counts: BTreeMap<String, u64>,
    pub typed_clone_rejection_reason_counts: BTreeMap<String, u64>,
    pub typed_path_decision_counts: BTreeMap<String, u64>,
    pub typed_path_selection_reason_counts: BTreeMap<String, u64>,
    pub typed_path_fallback_reason_counts: BTreeMap<String, u64>,
    pub typed_path_rejection_reason_counts: BTreeMap<String, u64>,
    pub collection_helper_decision_counts: BTreeMap<String, u64>,
    pub collection_helper_family_counts: BTreeMap<String, u64>,
    pub collection_helper_selection_reason_counts: BTreeMap<String, u64>,
    pub collection_helper_rejection_reason_counts: BTreeMap<String, u64>,
    pub collection_typed_value_decision_counts: BTreeMap<String, u64>,
    pub collection_typed_value_selection_reason_counts: BTreeMap<String, u64>,
    pub collection_typed_value_rejection_reason_counts: BTreeMap<String, u64>,
    pub generic_fallback_reason_counts: BTreeMap<String, u64>,
    pub dynamic_boundary_reason_counts: BTreeMap<String, u64>,
    pub box_reason_counts: BTreeMap<String, u64>,
    pub unbox_or_coercion_reason_counts: BTreeMap<String, u64>,
    pub runtime_property_get_reason_counts: BTreeMap<String, u64>,
    pub direct_field_load_reason_counts: BTreeMap<String, u64>,
    pub scalar_replacement_decision_counts: BTreeMap<String, u64>,
    pub scalar_replacement_selection_reason_counts: BTreeMap<String, u64>,
    pub scalar_replacement_rejection_reason_counts: BTreeMap<String, u64>,
    pub scalar_replacement_fallback_reason_counts: BTreeMap<String, u64>,
    pub scalar_replacement_reason_counts: BTreeMap<String, u64>,
    pub bounds_eliminated_reason_counts: BTreeMap<String, u64>,
    pub bounds_kept_reason_counts: BTreeMap<String, u64>,
    pub barrier_elimination_reason_counts: BTreeMap<String, u64>,
    pub barrier_emission_reason_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct LoweringEvidence {
    pub typed_clone_decisions: Vec<EvidenceRow>,
    pub dynamic_fallbacks: Vec<EvidenceRow>,
    pub boxes: Vec<EvidenceRow>,
    pub unboxes_or_coercions: Vec<EvidenceRow>,
    pub bounds_decisions: Vec<EvidenceRow>,
    pub barrier_decisions: Vec<EvidenceRow>,
    pub direct_field_loads: Vec<EvidenceRow>,
    pub runtime_property_gets: Vec<EvidenceRow>,
    pub scalar_replacements: Vec<EvidenceRow>,
    pub collection_helper_decisions: Vec<EvidenceRow>,
    pub collection_typed_value_decisions: Vec<EvidenceRow>,
    pub typed_path_decisions: Vec<EvidenceRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct EvidenceRow {
    pub module: String,
    pub function: Option<String>,
    pub expr_kind: Option<String>,
    pub consumer: Option<String>,
    pub native_rep: Option<String>,
    pub native_value_state: Option<String>,
    pub access_mode: Option<String>,
    pub materialization_reason: Option<String>,
    pub fallback_reason: Option<String>,
    pub decision: Option<String>,
    pub reason_category: Option<String>,
    pub typed_clone: Option<String>,
    pub generic_fallback: Option<String>,
    pub consumed_facts: Vec<String>,
    pub rejected_facts: Vec<String>,
    pub notes: Vec<String>,
}

pub(super) fn build_report_from_dir(dir: &Path) -> Result<ExplainLoweringReport> {
    let mut artifacts = Vec::new();
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read native-rep artifact dir {}", dir.display()))?
    {
        let path = entry?.path();
        if path.file_name().and_then(|n| n.to_str()) == Some("explain-lowering.json") {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read native-rep artifact {}", path.display()))?;
        let value = serde_json::from_str::<Value>(&raw)
            .with_context(|| format!("failed to parse native-rep artifact {}", path.display()))?;
        artifacts.push((path, value));
    }
    artifacts.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(build_report_from_artifacts(dir, artifacts))
}

fn build_report_from_artifacts(
    artifact_dir: &Path,
    artifacts: Vec<(PathBuf, Value)>,
) -> ExplainLoweringReport {
    let mut modules = BTreeSet::new();
    let mut summary = LoweringSummary::default();
    let mut evidence = LoweringEvidence::default();

    for (_path, artifact) in &artifacts {
        let module = artifact
            .get("module")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>")
            .to_string();
        modules.insert(module.clone());
        if let Some(records) = artifact.get("records").and_then(Value::as_array) {
            for record in records {
                aggregate_record(&module, record, &mut summary, &mut evidence);
            }
        }
        if let Some(summary_value) = artifact.get("summary") {
            summary.native_owned_views += summary_u64(summary_value, "native_owned_view_count");
            summary.pod_layouts += summary_u64(summary_value, "pod_layout_count");
            summary.pod_records += summary_u64(summary_value, "pod_record_count");
            summary.pod_record_views += summary_u64(summary_value, "pod_record_view_count");
            summary.pod_materializations += summary_u64(summary_value, "pod_materialization_count");
        }
    }

    ExplainLoweringReport {
        version: REPORT_VERSION,
        artifact_dir: artifact_dir.display().to_string(),
        report_path: String::new(),
        artifact_count: artifacts.len(),
        modules: modules.into_iter().collect(),
        summary,
        evidence,
    }
}

fn aggregate_record(
    module: &str,
    record: &Value,
    summary: &mut LoweringSummary,
    evidence: &mut LoweringEvidence,
) {
    summary.record_count += 1;

    if let Some(native_rep) = string_field(record, "native_rep_name") {
        increment(&mut summary.native_rep_counts, &native_rep);
        if native_rep == "js_value_bits" {
            summary.js_value_bits_records += 1;
        }
    }

    if let Some(state) = string_field(record, "native_value_state") {
        increment(&mut summary.native_value_state_counts, &state);
        if state == "dynamic_fallback" {
            summary.dynamic_fallbacks += 1;
        }
    }

    if let Some(mode) = string_field(record, "access_mode") {
        increment(&mut summary.access_mode_counts, &mode);
        if mode == "dynamic_fallback"
            && string_field(record, "native_value_state").as_deref() != Some("dynamic_fallback")
        {
            summary.dynamic_fallbacks += 1;
        }
    }

    let materialization_reason = string_field(record, "materialization_reason");
    let fallback_reason = string_field(record, "fallback_reason");

    if let Some(reason) = materialization_reason.as_deref() {
        increment(&mut summary.materialization_reason_counts, reason);
    }
    if let Some(reason) = fallback_reason.as_deref() {
        increment(&mut summary.fallback_reason_counts, reason);
    }

    let transition = record.get("native_abi_transition");
    let scalar_conversion = record.get("scalar_conversion");
    let boxes_inserted = materialization_reason.is_some()
        || transition
            .and_then(|value| string_field(value, "to_native_rep"))
            .as_deref()
            == Some("js_value");
    if boxes_inserted {
        summary.boxes_inserted += 1;
        let reason = box_reason(record, transition);
        increment(&mut summary.box_reason_counts, &reason);
        push_evidence(
            &mut evidence.boxes,
            module,
            record,
            Some("box_inserted".to_string()),
            Some(reason),
        );
    }

    if let Some(op) = transition.and_then(|value| string_field(value, "op")) {
        if is_unbox_or_coercion_op(&op) {
            summary.unboxes_or_coercions += 1;
            let reason = conversion_reason(record, transition);
            increment(&mut summary.unbox_or_coercion_reason_counts, &reason);
            push_evidence(
                &mut evidence.unboxes_or_coercions,
                module,
                record,
                Some(format!("unbox_or_coercion:{op}")),
                Some(reason),
            );
        }
        increment(&mut summary.scalar_conversion_counts, &op);
    }
    if let Some(op) = scalar_conversion.and_then(|value| string_field(value, "op")) {
        if is_unbox_or_coercion_op(&op) {
            summary.unboxes_or_coercions += 1;
            let reason = conversion_reason(record, scalar_conversion);
            increment(&mut summary.unbox_or_coercion_reason_counts, &reason);
            push_evidence(
                &mut evidence.unboxes_or_coercions,
                module,
                record,
                Some(format!("unbox_or_coercion:{op}")),
                Some(reason),
            );
        }
        increment(&mut summary.scalar_conversion_counts, &op);
    }

    let expr_kind = string_field(record, "expr_kind").unwrap_or_default();
    let consumer = string_field(record, "consumer").unwrap_or_default();
    let notes = notes(record);
    let notes_text = notes.join(";");
    let access_mode = string_field(record, "access_mode").unwrap_or_default();

    let is_dynamic_fallback =
        access_mode == "dynamic_fallback" || string_field(record, "fallback_reason").is_some();
    if is_dynamic_fallback {
        let reason = dynamic_boundary_reason(record);
        increment(&mut summary.dynamic_boundary_reason_counts, &reason);
        push_typed_path_evidence(
            summary,
            evidence,
            module,
            record,
            "fallback",
            format!("dynamic_boundary:{reason}"),
        );
        push_evidence(
            &mut evidence.dynamic_fallbacks,
            module,
            record,
            Some("dynamic_boundary".to_string()),
            Some(reason),
        );
    }

    if is_runtime_property_get(&expr_kind, &consumer, record) {
        summary.runtime_property_gets += 1;
        let reason = boundary_or_materialization_reason(record);
        increment(&mut summary.runtime_property_get_reason_counts, &reason);
        push_evidence(
            &mut evidence.runtime_property_gets,
            module,
            record,
            Some("runtime_property_get".to_string()),
            Some(reason),
        );
    }

    if is_direct_field_load(&expr_kind, &consumer, &access_mode) {
        summary.direct_field_loads += 1;
        let reason = direct_field_load_reason(record);
        increment(&mut summary.direct_field_load_reason_counts, &reason);
        push_evidence(
            &mut evidence.direct_field_loads,
            module,
            record,
            Some("direct_field_load".to_string()),
            Some(reason),
        );
    }

    if bounds_state_name(record.get("bounds_state")).as_deref() == Some("proven") {
        summary.bounds_eliminations += 1;
    }
    if let Some((decision, reason)) = bounds_decision(record) {
        match decision.as_str() {
            "bounds_eliminated" => increment(&mut summary.bounds_eliminated_reason_counts, &reason),
            "bounds_kept" => increment(&mut summary.bounds_kept_reason_counts, &reason),
            _ => {}
        }
        push_evidence(
            &mut evidence.bounds_decisions,
            module,
            record,
            Some(decision),
            Some(reason),
        );
    }

    if let Some(reason) = barrier_elimination_reason(&expr_kind, &consumer, &notes) {
        summary.barrier_eliminations += 1;
        increment(&mut summary.barrier_elimination_reason_counts, &reason);
        push_evidence(
            &mut evidence.barrier_decisions,
            module,
            record,
            Some("barrier_eliminated".to_string()),
            Some(reason),
        );
    }
    if let Some(reason) = barrier_emission_reason(&expr_kind, &consumer, &notes) {
        summary.barrier_emissions += 1;
        increment(&mut summary.barrier_emission_reason_counts, &reason);
        push_evidence(
            &mut evidence.barrier_decisions,
            module,
            record,
            Some("barrier_emitted".to_string()),
            Some(reason),
        );
    }

    if let Some((decision, reason)) = scalar_replacement_decision(record, &expr_kind, &consumer) {
        increment(&mut summary.scalar_replacement_decision_counts, &decision);
        match decision.as_str() {
            "selected" => {
                summary.scalar_replacements += 1;
                increment(
                    &mut summary.scalar_replacement_selection_reason_counts,
                    &reason,
                );
            }
            "fallback" => {
                summary.scalar_replacement_fallbacks += 1;
                increment(
                    &mut summary.scalar_replacement_fallback_reason_counts,
                    &reason,
                );
            }
            "rejected" => {
                summary.scalar_replacement_rejections += 1;
                increment(
                    &mut summary.scalar_replacement_rejection_reason_counts,
                    &reason,
                );
            }
            _ => {}
        }
        increment(&mut summary.scalar_replacement_reason_counts, &reason);
        push_evidence(
            &mut evidence.scalar_replacements,
            module,
            record,
            Some(format!("scalar_replacement_{decision}")),
            Some(reason),
        );
    }

    if let Some(family) = collection_helper_family(&consumer) {
        increment(&mut summary.collection_helper_family_counts, &family);
        if let Some(reason) = collection_helper_selection_reason(&consumer, &notes) {
            summary.collection_helper_selections += 1;
            increment(&mut summary.collection_helper_decision_counts, "selected");
            increment(
                &mut summary.collection_helper_selection_reason_counts,
                &reason,
            );
            push_evidence(
                &mut evidence.collection_helper_decisions,
                module,
                record,
                Some("collection_helper_selected".to_string()),
                Some(reason),
            );
        } else if let Some(reason) = collection_helper_rejection_reason(record, &notes) {
            summary.collection_helper_fallback_decisions += 1;
            summary.generic_fallback_emissions += 1;
            increment(&mut summary.collection_helper_decision_counts, "rejected");
            increment(
                &mut summary.collection_helper_rejection_reason_counts,
                &reason,
            );
            if let Some(generic_reason) = collection_generic_fallback_reason(&consumer, &notes) {
                increment(&mut summary.generic_fallback_reason_counts, &generic_reason);
            }
            push_evidence(
                &mut evidence.collection_helper_decisions,
                module,
                record,
                Some("collection_helper_rejected".to_string()),
                Some(reason),
            );
        }
    }

    if let Some(reason) = collection_typed_value_selection_reason(record, &notes) {
        summary.collection_typed_value_selections += 1;
        increment(
            &mut summary.collection_typed_value_decision_counts,
            "selected",
        );
        increment(
            &mut summary.collection_typed_value_selection_reason_counts,
            &reason,
        );
        push_evidence(
            &mut evidence.collection_typed_value_decisions,
            module,
            record,
            Some("collection_typed_value_selected".to_string()),
            Some(reason.clone()),
        );
        push_typed_path_evidence(
            summary,
            evidence,
            module,
            record,
            "selected",
            "collection_typed_value_selected".to_string(),
        );
    } else if let Some(reason) = collection_typed_value_rejection_reason(record, &notes) {
        summary.collection_typed_value_fallback_decisions += 1;
        increment(
            &mut summary.collection_typed_value_decision_counts,
            "rejected",
        );
        increment(
            &mut summary.collection_typed_value_rejection_reason_counts,
            &reason,
        );
        push_evidence(
            &mut evidence.collection_typed_value_decisions,
            module,
            record,
            Some("collection_typed_value_rejected".to_string()),
            Some(reason.clone()),
        );
        push_typed_path_evidence(
            summary,
            evidence,
            module,
            record,
            "rejected",
            format!("collection_typed_value:{reason}"),
        );
    }

    if typed_clone_name(&notes).is_some() {
        summary.typed_clone_selections += 1;
        increment(&mut summary.typed_clone_decision_counts, "selected");
        let reason = typed_clone_selection_reason(&consumer);
        increment(&mut summary.typed_clone_selection_reason_counts, &reason);
        push_typed_path_evidence(
            summary,
            evidence,
            module,
            record,
            "selected",
            format!("typed_clone:{reason}"),
        );
        if let Some(reason) = generic_fallback_reason(record, &notes) {
            summary.generic_fallback_emissions += 1;
            increment(&mut summary.generic_fallback_reason_counts, &reason);
            push_typed_path_evidence(
                summary,
                evidence,
                module,
                record,
                "fallback",
                format!("generic_fallback:{reason}"),
            );
        }
        if generic_fallback_name(&notes).is_some() || notes_text.contains("fallback") {
            summary.typed_clone_fallback_decisions += 1;
        }
        push_evidence(
            &mut evidence.typed_clone_decisions,
            module,
            record,
            Some("typed_clone_selected".to_string()),
            Some(reason),
        );
    } else if let Some(reason) = typed_clone_rejection_reason(record, &notes) {
        increment(&mut summary.typed_clone_decision_counts, "rejected");
        increment(&mut summary.typed_clone_rejection_reason_counts, &reason);
        push_typed_path_evidence(
            summary,
            evidence,
            module,
            record,
            "rejected",
            format!("typed_clone:{reason}"),
        );
        push_evidence(
            &mut evidence.typed_clone_decisions,
            module,
            record,
            Some("typed_clone_rejected".to_string()),
            Some(reason),
        );
    } else if is_dynamic_fallback {
        increment(&mut summary.typed_clone_decision_counts, NOT_RECORDED);
    }
}

fn print_text_report(report: &ExplainLoweringReport) {
    let summary = &report.summary;
    println!();
    println!("Type lowering report");
    println!("  report: {}", report.report_path);
    println!(
        "  artifacts: {}  modules: {}  records: {}",
        report.artifact_count,
        report.modules.len(),
        summary.record_count
    );
    println!(
        "  boxes: {}  unboxes/coercions: {}  dynamic fallbacks: {}",
        summary.boxes_inserted, summary.unboxes_or_coercions, summary.dynamic_fallbacks
    );
    println!(
        "  JSValueBits: {}  typed clones: {}  clone fallbacks: {}",
        summary.js_value_bits_records,
        summary.typed_clone_selections,
        summary.typed_clone_fallback_decisions
    );
    println!(
        "  typed paths: {} selected  {} fallback  {} rejected",
        summary.typed_path_selections, summary.typed_path_fallbacks, summary.typed_path_rejections
    );
    println!(
        "  runtime property gets: {}  direct field loads: {}  bounds eliminations: {}",
        summary.runtime_property_gets, summary.direct_field_loads, summary.bounds_eliminations
    );
    println!(
        "  barrier eliminations: {}  barrier emissions: {}  scalar replacements: {}",
        summary.barrier_eliminations, summary.barrier_emissions, summary.scalar_replacements
    );
    if summary.scalar_replacement_fallbacks > 0 || summary.scalar_replacement_rejections > 0 {
        println!(
            "  scalar replacement fallbacks: {}  rejections: {}",
            summary.scalar_replacement_fallbacks, summary.scalar_replacement_rejections
        );
    }
    println!(
        "  collection helpers: {} selected  {} rejected/generic",
        summary.collection_helper_selections, summary.collection_helper_fallback_decisions
    );
    println!(
        "  collection typed values: {} selected  {} rejected/generic",
        summary.collection_typed_value_selections,
        summary.collection_typed_value_fallback_decisions
    );

    if !summary.native_rep_counts.is_empty() {
        println!(
            "  native reps: {}",
            format_counts(&summary.native_rep_counts)
        );
    }
    if !summary.fallback_reason_counts.is_empty() {
        println!(
            "  fallback reasons: {}",
            format_counts(&summary.fallback_reason_counts)
        );
    }
    if !summary.materialization_reason_counts.is_empty() {
        println!(
            "  materialization reasons: {}",
            format_counts(&summary.materialization_reason_counts)
        );
    }
    if !summary.typed_clone_decision_counts.is_empty() {
        println!(
            "  typed clone decisions: {}",
            format_counts(&summary.typed_clone_decision_counts)
        );
    }
    if !summary.typed_clone_selection_reason_counts.is_empty() {
        println!(
            "  typed clone selection reasons: {}",
            format_counts(&summary.typed_clone_selection_reason_counts)
        );
    }
    if !summary.typed_clone_rejection_reason_counts.is_empty() {
        println!(
            "  typed clone rejection reasons: {}",
            format_counts(&summary.typed_clone_rejection_reason_counts)
        );
    }
    if !summary.typed_path_decision_counts.is_empty() {
        println!(
            "  typed path decisions: {}",
            format_counts(&summary.typed_path_decision_counts)
        );
    }
    if !summary.typed_path_selection_reason_counts.is_empty() {
        println!(
            "  typed path selection reasons: {}",
            format_counts(&summary.typed_path_selection_reason_counts)
        );
    }
    if !summary.typed_path_fallback_reason_counts.is_empty() {
        println!(
            "  typed path fallback reasons: {}",
            format_counts(&summary.typed_path_fallback_reason_counts)
        );
    }
    if !summary.typed_path_rejection_reason_counts.is_empty() {
        println!(
            "  typed path rejection reasons: {}",
            format_counts(&summary.typed_path_rejection_reason_counts)
        );
    }
    if !summary.collection_helper_decision_counts.is_empty() {
        println!(
            "  collection helper decisions: {}",
            format_counts(&summary.collection_helper_decision_counts)
        );
    }
    if !summary.collection_helper_family_counts.is_empty() {
        println!(
            "  collection helper families: {}",
            format_counts(&summary.collection_helper_family_counts)
        );
    }
    if !summary.collection_helper_selection_reason_counts.is_empty() {
        println!(
            "  collection helper selection reasons: {}",
            format_counts(&summary.collection_helper_selection_reason_counts)
        );
    }
    if !summary.collection_helper_rejection_reason_counts.is_empty() {
        println!(
            "  collection helper rejection reasons: {}",
            format_counts(&summary.collection_helper_rejection_reason_counts)
        );
    }
    if !summary.collection_typed_value_decision_counts.is_empty() {
        println!(
            "  collection typed value decisions: {}",
            format_counts(&summary.collection_typed_value_decision_counts)
        );
    }
    if !summary
        .collection_typed_value_selection_reason_counts
        .is_empty()
    {
        println!(
            "  collection typed value selection reasons: {}",
            format_counts(&summary.collection_typed_value_selection_reason_counts)
        );
    }
    if !summary
        .collection_typed_value_rejection_reason_counts
        .is_empty()
    {
        println!(
            "  collection typed value rejection reasons: {}",
            format_counts(&summary.collection_typed_value_rejection_reason_counts)
        );
    }
    if !summary.generic_fallback_reason_counts.is_empty() {
        println!(
            "  generic fallback reasons: {}",
            format_counts(&summary.generic_fallback_reason_counts)
        );
    }
    if !summary.dynamic_boundary_reason_counts.is_empty() {
        println!(
            "  dynamic boundary reasons: {}",
            format_counts(&summary.dynamic_boundary_reason_counts)
        );
    }
    if !summary.box_reason_counts.is_empty() {
        println!(
            "  box reasons: {}",
            format_counts(&summary.box_reason_counts)
        );
    }
    if !summary.unbox_or_coercion_reason_counts.is_empty() {
        println!(
            "  unbox/coercion reasons: {}",
            format_counts(&summary.unbox_or_coercion_reason_counts)
        );
    }
    if !summary.scalar_replacement_reason_counts.is_empty() {
        println!(
            "  scalar replacement reasons: {}",
            format_counts(&summary.scalar_replacement_reason_counts)
        );
    }
    if !summary.scalar_replacement_decision_counts.is_empty() {
        println!(
            "  scalar replacement decisions: {}",
            format_counts(&summary.scalar_replacement_decision_counts)
        );
    }
    if !summary
        .scalar_replacement_selection_reason_counts
        .is_empty()
    {
        println!(
            "  scalar replacement selection reasons: {}",
            format_counts(&summary.scalar_replacement_selection_reason_counts)
        );
    }
    if !summary.scalar_replacement_fallback_reason_counts.is_empty() {
        println!(
            "  scalar replacement fallback reasons: {}",
            format_counts(&summary.scalar_replacement_fallback_reason_counts)
        );
    }
    if !summary
        .scalar_replacement_rejection_reason_counts
        .is_empty()
    {
        println!(
            "  scalar replacement rejection reasons: {}",
            format_counts(&summary.scalar_replacement_rejection_reason_counts)
        );
    }
    if !summary.bounds_eliminated_reason_counts.is_empty() {
        println!(
            "  bounds eliminated reasons: {}",
            format_counts(&summary.bounds_eliminated_reason_counts)
        );
    }
    if !summary.bounds_kept_reason_counts.is_empty() {
        println!(
            "  bounds kept reasons: {}",
            format_counts(&summary.bounds_kept_reason_counts)
        );
    }
    if !summary.barrier_elimination_reason_counts.is_empty() {
        println!(
            "  barrier eliminated reasons: {}",
            format_counts(&summary.barrier_elimination_reason_counts)
        );
    }
    if !summary.barrier_emission_reason_counts.is_empty() {
        println!(
            "  barrier emitted reasons: {}",
            format_counts(&summary.barrier_emission_reason_counts)
        );
    }
}

fn format_counts(counts: &BTreeMap<String, u64>) -> String {
    counts
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn push_evidence(
    rows: &mut Vec<EvidenceRow>,
    module: &str,
    record: &Value,
    decision: Option<String>,
    reason_category: Option<String>,
) {
    if rows.len() >= MAX_EVIDENCE_ROWS {
        return;
    }
    let notes = notes(record);
    rows.push(EvidenceRow {
        module: module.to_string(),
        function: string_field(record, "source_function")
            .or_else(|| string_field(record, "function")),
        expr_kind: string_field(record, "expr_kind"),
        consumer: string_field(record, "consumer"),
        native_rep: string_field(record, "native_rep_name"),
        native_value_state: string_field(record, "native_value_state"),
        access_mode: string_field(record, "access_mode"),
        materialization_reason: string_field(record, "materialization_reason"),
        fallback_reason: string_field(record, "fallback_reason"),
        decision,
        reason_category,
        typed_clone: typed_clone_name(&notes),
        generic_fallback: generic_fallback_name(&notes),
        consumed_facts: fact_labels(record, "consumed_facts"),
        rejected_facts: fact_labels(record, "rejected_facts"),
        notes,
    });
}

fn push_typed_path_evidence(
    summary: &mut LoweringSummary,
    evidence: &mut LoweringEvidence,
    module: &str,
    record: &Value,
    decision: &str,
    reason: String,
) {
    match decision {
        "selected" => {
            summary.typed_path_selections += 1;
            increment(&mut summary.typed_path_selection_reason_counts, &reason);
        }
        "fallback" => {
            summary.typed_path_fallbacks += 1;
            increment(&mut summary.typed_path_fallback_reason_counts, &reason);
        }
        "rejected" => {
            summary.typed_path_rejections += 1;
            increment(&mut summary.typed_path_rejection_reason_counts, &reason);
        }
        _ => {}
    }
    increment(&mut summary.typed_path_decision_counts, decision);
    push_evidence(
        &mut evidence.typed_path_decisions,
        module,
        record,
        Some(format!("typed_path_{decision}")),
        Some(reason),
    );
}

fn increment(counts: &mut BTreeMap<String, u64>, key: &str) {
    *counts.entry(key.to_string()).or_insert(0) += 1;
}

fn summary_u64(summary: &Value, key: &str) -> u64 {
    summary.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(value_string)
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(map) => {
            if let Some(kind) = map.get("kind").and_then(Value::as_str) {
                return Some(kind.to_string());
            }
            if map.len() == 1 {
                return map.keys().next().cloned();
            }
            None
        }
        _ => None,
    }
}

fn notes(record: &Value) -> Vec<String> {
    record
        .get("notes")
        .and_then(Value::as_array)
        .map(|notes| {
            notes
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn fact_labels(record: &Value, field: &str) -> Vec<String> {
    record
        .get(field)
        .and_then(Value::as_array)
        .map(|facts| {
            facts
                .iter()
                .filter_map(|fact| {
                    let fact_id = string_field(fact, "fact_id")?;
                    let state =
                        string_field(fact, "state").unwrap_or_else(|| NOT_RECORDED.to_string());
                    let reason = string_field(fact, "reason")
                        .or_else(|| string_field(fact, "detail"))
                        .unwrap_or_else(|| NOT_RECORDED.to_string());
                    Some(format!("{fact_id}:{state}:{reason}"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn note_value(notes: &[String], key: &str) -> Option<String> {
    for note in notes {
        for part in note.split(';') {
            let part = part.trim();
            if let Some(value) = part
                .strip_prefix(key)
                .and_then(|value| value.strip_prefix('='))
            {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn typed_clone_name(notes: &[String]) -> Option<String> {
    note_value(notes, "typed_clone")
}

fn generic_fallback_name(notes: &[String]) -> Option<String> {
    note_value(notes, "generic_wrapper")
        .or_else(|| note_value(notes, "generic_method"))
        .or_else(|| note_value(notes, "generic_closure"))
}

fn generic_fallback_reason(record: &Value, notes: &[String]) -> Option<String> {
    if note_value(notes, "generic_wrapper").is_some() {
        return Some("generic_wrapper".to_string());
    }
    if note_value(notes, "generic_method").is_some() {
        return Some("generic_method".to_string());
    }
    if note_value(notes, "generic_closure").is_some() {
        return Some("generic_closure".to_string());
    }
    if notes.iter().any(|note| note.contains("fallback")) {
        return Some("fallback_note".to_string());
    }
    if is_dynamic_boundary_record(record) {
        return Some(dynamic_boundary_reason(record));
    }
    None
}

fn collection_helper_family(consumer: &str) -> Option<String> {
    consumer
        .strip_prefix("collection_string_key.")
        .map(|_| "collection_string_key".to_string())
        .or_else(|| {
            consumer
                .strip_prefix("collection_typed_value.")
                .map(|_| "collection_typed_value".to_string())
        })
}

fn collection_helper_selection_reason(consumer: &str, notes: &[String]) -> Option<String> {
    let helper = note_value(notes, "selected_helper")?;
    Some(format!("{consumer}:{helper}"))
}

fn collection_helper_rejection_reason(record: &Value, notes: &[String]) -> Option<String> {
    note_value(notes, "typed_collection_rejected")
        .or_else(|| native_fact_reason(record, "rejected_facts", "type_fact"))
        .map(|reason| {
            let helper =
                note_value(notes, "generic_helper").unwrap_or_else(|| "generic".to_string());
            format!("{reason}:{helper}")
        })
}

fn collection_generic_fallback_reason(consumer: &str, notes: &[String]) -> Option<String> {
    note_value(notes, "generic_helper").map(|helper| format!("{consumer}:{helper}"))
}

fn collection_typed_value_selection_reason(record: &Value, notes: &[String]) -> Option<String> {
    let (fact_id, _) = collection_typed_value_fact(record, "consumed_facts", "consumed")?;
    let helper = note_value(notes, "selected_helper").unwrap_or_else(|| "selected".to_string());
    Some(format!("{fact_id}:{helper}"))
}

fn collection_typed_value_rejection_reason(record: &Value, notes: &[String]) -> Option<String> {
    let (fact_id, fact_reason) = collection_typed_value_fact(record, "rejected_facts", "rejected")?;
    let reason = note_value(notes, "typed_collection_rejected")
        .or(fact_reason)
        .unwrap_or_else(|| NOT_RECORDED.to_string());
    let helper = note_value(notes, "generic_helper").unwrap_or_else(|| "generic".to_string());
    Some(format!("{fact_id}:{reason}:{helper}"))
}

fn collection_typed_value_fact(
    record: &Value,
    field: &str,
    state: &str,
) -> Option<(String, Option<String>)> {
    record
        .get(field)
        .and_then(Value::as_array)?
        .iter()
        .find_map(|fact| {
            if string_field(fact, "kind").as_deref() != Some("type_fact") {
                return None;
            }
            if string_field(fact, "state").as_deref() != Some(state) {
                return None;
            }
            let fact_id = string_field(fact, "fact_id")?;
            if !matches!(fact_id.split_once('.'), Some(("map" | "set", _)))
                || !fact_id.ends_with("_value_helper")
            {
                return None;
            }
            Some((fact_id, string_field(fact, "reason")))
        })
}

fn typed_clone_selection_reason(consumer: &str) -> String {
    match consumer {
        "typed_f64_func_ref_call" => "typed_f64_function_direct_call",
        "typed_i32_func_ref_call" => "typed_i32_function_direct_call",
        "typed_i1_func_ref_call" => "typed_i1_function_direct_call",
        "typed_f64_method_direct_call" => "typed_f64_method_direct_call",
        "typed_i1_method_direct_call" => "typed_i1_method_direct_call",
        "typed_f64_closure_direct_call" => "typed_f64_closure_direct_call",
        "typed_i1_closure_direct_call" => "typed_i1_closure_direct_call",
        _ if consumer.contains("typed_f64") => "typed_f64_artifact_consumer",
        _ if consumer.contains("typed_i32") => "typed_i32_artifact_consumer",
        _ if consumer.contains("typed_i1") => "typed_i1_artifact_consumer",
        _ => "typed_clone_artifact_note",
    }
    .to_string()
}

fn typed_clone_rejection_reason(record: &Value, notes: &[String]) -> Option<String> {
    note_value(notes, "typed_clone_rejected")
        .or_else(|| note_value(notes, "typed_clone_rejection"))
        .or_else(|| native_fact_reason(record, "rejected_facts", "typed_clone"))
}

fn native_fact_reason(record: &Value, field: &str, kind_prefix: &str) -> Option<String> {
    record
        .get(field)
        .and_then(Value::as_array)?
        .iter()
        .find_map(|fact| {
            let kind = string_field(fact, "kind")?;
            if !kind.starts_with(kind_prefix) {
                return None;
            }
            string_field(fact, "reason")
                .or_else(|| string_field(fact, "detail"))
                .or_else(|| string_field(fact, "state"))
                .or_else(|| Some(NOT_RECORDED.to_string()))
        })
}

fn boundary_or_materialization_reason(record: &Value) -> String {
    string_field(record, "fallback_reason")
        .or_else(|| string_field(record, "materialization_reason"))
        .unwrap_or_else(|| NOT_RECORDED.to_string())
}

fn dynamic_boundary_reason(record: &Value) -> String {
    boundary_or_materialization_reason(record)
}

fn box_reason(record: &Value, transition: Option<&Value>) -> String {
    string_field(record, "materialization_reason")
        .or_else(|| transition.and_then(|value| string_field(value, "reason")))
        .unwrap_or_else(|| NOT_RECORDED.to_string())
}

fn conversion_reason(record: &Value, conversion: Option<&Value>) -> String {
    conversion
        .and_then(|value| string_field(value, "reason"))
        .or_else(|| string_field(record, "materialization_reason"))
        .unwrap_or_else(|| NOT_RECORDED.to_string())
}

fn bounds_state_name(value: Option<&Value>) -> Option<String> {
    value.and_then(value_string)
}

fn bounds_decision(record: &Value) -> Option<(String, String)> {
    let access_mode = string_field(record, "access_mode");
    let Some(bounds) = record.get("bounds_state") else {
        if matches!(
            access_mode.as_deref(),
            Some("checked_native" | "dynamic_fallback")
        ) {
            return Some(("bounds_kept".to_string(), bounds_kept_reason(record)));
        }
        return None;
    };
    match bounds {
        Value::Object(map) => {
            if let Some(proven) = map.get("proven") {
                let reason =
                    string_field(proven, "proof").unwrap_or_else(|| NOT_RECORDED.to_string());
                return Some(("bounds_eliminated".to_string(), reason));
            }
            if let Some(guarded) = map.get("guarded") {
                let reason = string_field(guarded, "guard_id")
                    .map(|guard| format!("guarded:{guard}"))
                    .unwrap_or_else(|| "guarded:not_recorded".to_string());
                return Some(("bounds_eliminated".to_string(), reason));
            }
            if map.contains_key("unknown") {
                return Some(("bounds_kept".to_string(), bounds_kept_reason(record)));
            }
        }
        Value::String(value) if value == "unknown" => {
            return Some(("bounds_kept".to_string(), bounds_kept_reason(record)));
        }
        _ => {}
    }

    if matches!(
        access_mode.as_deref(),
        Some("checked_native" | "dynamic_fallback")
    ) {
        return Some(("bounds_kept".to_string(), bounds_kept_reason(record)));
    }
    None
}

fn bounds_kept_reason(record: &Value) -> String {
    string_field(record, "fallback_reason")
        .or_else(|| string_field(record, "materialization_reason"))
        .unwrap_or_else(|| "unknown_bounds".to_string())
}

fn direct_field_load_reason(record: &Value) -> String {
    let notes = notes(record);
    native_fact_reason(record, "consumed_facts", "raw_f64_layout")
        .map(|reason| format!("raw_f64_layout:{reason}"))
        .or_else(|| {
            if note_value(&notes, "raw_f64_field").as_deref() == Some("1")
                || string_field(record, "consumer")
                    .as_deref()
                    .is_some_and(|consumer| consumer.contains("scalar_object_field_load.raw_f64"))
            {
                Some("scalar_replacement_raw_f64_field".to_string())
            } else {
                None
            }
        })
        .or_else(|| {
            let expr_kind = string_field(record, "expr_kind").unwrap_or_default();
            let consumer = string_field(record, "consumer").unwrap_or_default();
            if expr_kind.starts_with("Scalar") || consumer.starts_with("scalar_object_") {
                Some("scalar_replacement_field_load".to_string())
            } else if consumer.contains("raw_f64") {
                Some("raw_f64_field_consumer".to_string())
            } else {
                None
            }
        })
        .or_else(|| string_field(record, "access_mode"))
        .unwrap_or_else(|| NOT_RECORDED.to_string())
}

fn scalar_replacement_reason(record: &Value) -> String {
    let notes = notes(record);
    native_fact_reason(record, "consumed_facts", "scalar_method_summary")
        .map(|reason| format!("scalar_method_summary:{reason}"))
        .or_else(|| {
            native_fact_reason(record, "rejected_facts", "scalar_method_summary")
                .map(|reason| format!("scalar_method_materialized_fallback:{reason}"))
        })
        .or_else(|| {
            note_value(&notes, "scalar_method_fallback")
                .map(|reason| format!("scalar_method_materialized_fallback:{reason}"))
        })
        .or_else(|| {
            let direct_reason = direct_field_load_reason(record);
            (direct_reason != NOT_RECORDED).then_some(direct_reason)
        })
        .or_else(|| {
            let consumer = string_field(record, "consumer").unwrap_or_default();
            match consumer.as_str() {
                "scalar_method_summary_inline" => Some("scalar_method_summary_inline".to_string()),
                "scalar_method_summary_materialized_fallback" => {
                    Some("scalar_method_materialized_fallback".to_string())
                }
                _ => None,
            }
        })
        .unwrap_or_else(|| NOT_RECORDED.to_string())
}

fn scalar_replacement_decision(
    record: &Value,
    expr_kind: &str,
    consumer: &str,
) -> Option<(String, String)> {
    let reason = scalar_replacement_reason(record);
    let decision = match consumer {
        "scalar_method_summary_inline" => "selected",
        "scalar_method_summary_materialized_fallback" | "scalar_method_summary_fallback" => {
            "fallback"
        }
        "scalar_method_summary_rejected" => "rejected",
        _ if expr_kind.starts_with("Scalar") || consumer.starts_with("scalar_object_") => {
            "selected"
        }
        _ => return None,
    };
    Some((decision.to_string(), reason))
}

fn barrier_elimination_reason(
    _expr_kind: &str,
    _consumer: &str,
    notes: &[String],
) -> Option<String> {
    note_value(notes, "barrier_eliminated")
        .or_else(|| {
            notes
                .iter()
                .find(|note| note.contains("barrier_eliminated"))
                .map(|_| "barrier_eliminated_note".to_string())
        })
        .or_else(|| {
            notes
                .iter()
                .find(|note| note.contains("barrier=elided"))
                .map(|_| "barrier_elided".to_string())
        })
        .or_else(|| {
            notes
                .iter()
                .find(|note| note.contains("write_barrier=0"))
                .map(|_| "write_barrier=0".to_string())
        })
        .or_else(|| {
            notes
                .iter()
                .find(|note| note.contains("without_barrier"))
                .map(|_| "without_barrier".to_string())
        })
}

fn barrier_emission_reason(expr_kind: &str, consumer: &str, notes: &[String]) -> Option<String> {
    if barrier_elimination_reason(expr_kind, consumer, notes).is_some() {
        return None;
    }
    note_value(notes, "barrier_emitted")
        .or_else(|| {
            notes
                .iter()
                .find(|note| note.contains("barrier=emitted"))
                .map(|_| "barrier_emitted_note".to_string())
        })
        .or_else(|| {
            notes
                .iter()
                .find(|note| note.contains("write_barrier=1"))
                .map(|_| "write_barrier=1".to_string())
        })
        .or_else(|| {
            if consumer == "write_barrier.child_bits" {
                Some("maybe_pointer_child".to_string())
            } else if consumer.contains("write_barrier_slot") {
                Some("heap_slot_store_maybe_pointer_child".to_string())
            } else if consumer.contains("write_barrier_root") {
                Some("root_store_maybe_pointer_child".to_string())
            } else if expr_kind == "WriteBarrier" || consumer.contains("write_barrier") {
                Some("write_barrier_record".to_string())
            } else {
                None
            }
        })
}

fn is_dynamic_boundary_record(record: &Value) -> bool {
    string_field(record, "access_mode").as_deref() == Some("dynamic_fallback")
        || string_field(record, "native_value_state").as_deref() == Some("dynamic_fallback")
        || string_field(record, "fallback_reason").is_some()
}

fn is_unbox_or_coercion_op(op: &str) -> bool {
    matches!(
        op,
        "js_value_to_bits"
            | "bits_to_js_value"
            | "signed_int_to_float"
            | "unsigned_int_to_float"
            | "float_extend"
    )
}

fn is_runtime_property_get(expr_kind: &str, consumer: &str, record: &Value) -> bool {
    expr_kind.contains("PropertyGet")
        && (consumer.contains("runtime")
            || consumer.starts_with("js_")
            || string_field(record, "access_mode").as_deref() == Some("dynamic_fallback")
            || string_field(record, "materialization_reason").as_deref() == Some("runtime_api"))
}

fn is_direct_field_load(expr_kind: &str, consumer: &str, access_mode: &str) -> bool {
    (expr_kind == "ClassFieldGet" || expr_kind.ends_with("FieldGet"))
        && (consumer.contains("raw_f64_load")
            || consumer.contains("field_load")
            || consumer.contains("direct")
            || matches!(access_mode, "checked_native" | "unchecked_native"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static EXPLAIN_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn prepare_enables_and_restores_comprehensive_typed_clone_rejection_records() {
        let _guard = EXPLAIN_ENV_LOCK.lock().unwrap();
        let old_native_reps = std::env::var_os("PERRY_NATIVE_REPS");
        let old_native_reps_dir = std::env::var_os("PERRY_NATIVE_REPS_DIR");
        let old_all_rejections = std::env::var_os(ALL_TYPED_CLONE_REJECTIONS_ENV);
        std::env::set_var("PERRY_NATIVE_REPS", "old-reps");
        std::env::set_var("PERRY_NATIVE_REPS_DIR", "old-reps-dir");
        std::env::set_var(ALL_TYPED_CLONE_REJECTIONS_ENV, "old-all");

        let cache_root = std::env::temp_dir().join(format!(
            "perry_explain_lowering_env_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&cache_root);
        std::fs::create_dir_all(&cache_root).unwrap();

        {
            let _run = ExplainLoweringRun::prepare(&cache_root).unwrap();
            assert_eq!(std::env::var("PERRY_NATIVE_REPS").as_deref(), Ok("1"));
            assert_eq!(
                std::env::var(ALL_TYPED_CLONE_REJECTIONS_ENV).as_deref(),
                Ok("1")
            );
        }

        assert_eq!(
            std::env::var("PERRY_NATIVE_REPS").as_deref(),
            Ok("old-reps")
        );
        assert_eq!(
            std::env::var("PERRY_NATIVE_REPS_DIR").as_deref(),
            Ok("old-reps-dir")
        );
        assert_eq!(
            std::env::var(ALL_TYPED_CLONE_REJECTIONS_ENV).as_deref(),
            Ok("old-all")
        );

        match old_native_reps {
            Some(value) => std::env::set_var("PERRY_NATIVE_REPS", value),
            None => std::env::remove_var("PERRY_NATIVE_REPS"),
        }
        match old_native_reps_dir {
            Some(value) => std::env::set_var("PERRY_NATIVE_REPS_DIR", value),
            None => std::env::remove_var("PERRY_NATIVE_REPS_DIR"),
        }
        match old_all_rejections {
            Some(value) => std::env::set_var(ALL_TYPED_CLONE_REJECTIONS_ENV, value),
            None => std::env::remove_var(ALL_TYPED_CLONE_REJECTIONS_ENV),
        }
        let _ = std::fs::remove_dir_all(&cache_root);
    }

    #[test]
    fn report_counts_typed_clone_fallback_and_native_reps() {
        let artifact = json!({
            "schema_version": 14,
            "module": "typed.ts",
            "records": [
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "Call",
                    "consumer": "typed_f64_func_ref_call",
                    "native_rep_name": "f64",
                    "native_value_state": "region_local",
                    "notes": ["typed_clone=perry_fn_typed__add__typed_f64; generic_wrapper=perry_fn_typed__add"]
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "IndexGet",
                    "consumer": "js_typed_feedback_array_index_get_fallback_boxed",
                    "native_rep_name": "js_value",
                    "native_value_state": "dynamic_fallback",
                    "access_mode": "dynamic_fallback",
                    "bounds_state": "unknown",
                    "materialization_reason": "runtime_api",
                    "fallback_reason": "runtime_api",
                    "notes": []
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "Param",
                    "consumer": "function_param.js_value_bits",
                    "native_rep_name": "js_value_bits",
                    "native_value_state": "materialized",
                    "native_abi_transition": {
                        "from_native_rep": "js_value",
                        "to_native_rep": "js_value_bits",
                        "op": "js_value_to_bits",
                        "reason": "function_abi",
                        "lossy": false
                    },
                    "notes": []
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "WriteBarrier",
                    "consumer": "write_barrier.child_bits",
                    "native_rep_name": "js_value_bits",
                    "native_value_state": "region_local",
                    "notes": []
                }
            ],
            "summary": {
                "native_owned_view_count": 0,
                "pod_layout_count": 0,
                "pod_record_count": 0,
                "pod_record_view_count": 0,
                "pod_materialization_count": 0
            }
        });
        let report = build_report_from_artifacts(
            Path::new("/tmp/lowering"),
            vec![(PathBuf::from("native-reps.json"), artifact)],
        );

        assert_eq!(report.summary.typed_clone_selections, 1);
        assert_eq!(report.summary.typed_clone_fallback_decisions, 1);
        assert_eq!(report.summary.generic_fallback_emissions, 1);
        assert_eq!(report.summary.dynamic_fallbacks, 1);
        assert_eq!(report.summary.js_value_bits_records, 2);
        assert_eq!(report.summary.native_rep_counts.get("f64"), Some(&1));
        assert_eq!(report.summary.native_rep_counts.get("js_value"), Some(&1));
        assert_eq!(
            report.summary.native_rep_counts.get("js_value_bits"),
            Some(&2)
        );
        assert_eq!(
            report.summary.fallback_reason_counts.get("runtime_api"),
            Some(&1)
        );
        assert_eq!(
            report.summary.typed_clone_decision_counts.get("selected"),
            Some(&1)
        );
        assert_eq!(report.summary.typed_path_selections, 1);
        assert_eq!(report.summary.typed_path_fallbacks, 2);
        assert_eq!(
            report.summary.typed_path_decision_counts.get("selected"),
            Some(&1)
        );
        assert_eq!(
            report.summary.typed_path_decision_counts.get("fallback"),
            Some(&2)
        );
        assert_eq!(
            report
                .summary
                .typed_path_selection_reason_counts
                .get("typed_clone:typed_f64_function_direct_call"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .typed_path_fallback_reason_counts
                .get("generic_fallback:generic_wrapper"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .typed_path_fallback_reason_counts
                .get("dynamic_boundary:runtime_api"),
            Some(&1)
        );
        assert_eq!(
            report.summary.typed_clone_decision_counts.get(NOT_RECORDED),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .typed_clone_selection_reason_counts
                .get("typed_f64_function_direct_call"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .generic_fallback_reason_counts
                .get("generic_wrapper"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .dynamic_boundary_reason_counts
                .get("runtime_api"),
            Some(&1)
        );
        assert_eq!(
            report.summary.box_reason_counts.get("runtime_api"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .unbox_or_coercion_reason_counts
                .get("function_abi"),
            Some(&1)
        );
        assert_eq!(
            report.summary.bounds_kept_reason_counts.get("runtime_api"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .barrier_emission_reason_counts
                .get("maybe_pointer_child"),
            Some(&1)
        );
        assert_eq!(report.evidence.typed_clone_decisions.len(), 1);
        assert_eq!(report.evidence.dynamic_fallbacks.len(), 1);
        assert_eq!(
            report.evidence.typed_clone_decisions[0].decision.as_deref(),
            Some("typed_clone_selected")
        );
        assert_eq!(
            report.evidence.typed_clone_decisions[0]
                .reason_category
                .as_deref(),
            Some("typed_f64_function_direct_call")
        );
        assert_eq!(
            report.evidence.typed_clone_decisions[0]
                .typed_clone
                .as_deref(),
            Some("perry_fn_typed__add__typed_f64")
        );
        assert_eq!(
            report.evidence.typed_clone_decisions[0]
                .generic_fallback
                .as_deref(),
            Some("perry_fn_typed__add")
        );
        assert_eq!(
            report.evidence.dynamic_fallbacks[0]
                .reason_category
                .as_deref(),
            Some("runtime_api")
        );

        let json = serde_json::to_value(&report).unwrap();
        assert!(json["summary"]["typed_clone_selection_reason_counts"].is_object());
        assert!(json["summary"]["typed_path_decision_counts"].is_object());
        assert!(json["summary"]["dynamic_boundary_reason_counts"].is_object());
        assert!(json["summary"]["box_reason_counts"].is_object());
        assert!(json["summary"]["unbox_or_coercion_reason_counts"].is_object());
        assert!(json["evidence"]["typed_clone_decisions"][0]["typed_clone"].is_string());
        assert!(json["evidence"]["typed_path_decisions"].is_array());
    }

    #[test]
    fn report_counts_typed_i1_selection_and_rejection_reasons() {
        let artifact = json!({
            "schema_version": 14,
            "module": "typed.ts",
            "records": [
                {
                    "function": "caller",
                    "source_function": "caller",
                    "expr_kind": "Call",
                    "consumer": "typed_i1_func_ref_call",
                    "native_rep_name": "js_value",
                    "native_value_state": "region_local",
                    "notes": ["typed_clone=perry_fn_typed__both__typed_i1"]
                },
                {
                    "function": "both",
                    "source_function": "both",
                    "expr_kind": "TypedCloneDecision",
                    "consumer": "typed_i1_function_clone_decision",
                    "native_rep_name": "js_value",
                    "native_value_state": "region_local",
                    "notes": [
                        "typed_clone_rejected=param_not_i1",
                        "typed_clone_kind=typed_i1_function"
                    ]
                }
            ]
        });
        let report = build_report_from_artifacts(
            Path::new("/tmp/lowering"),
            vec![(PathBuf::from("native-reps.json"), artifact)],
        );

        assert_eq!(
            report.summary.typed_clone_decision_counts.get("selected"),
            Some(&1)
        );
        assert_eq!(
            report.summary.typed_clone_decision_counts.get("rejected"),
            Some(&1)
        );
        assert_eq!(report.summary.typed_path_selections, 1);
        assert_eq!(report.summary.typed_path_rejections, 1);
        assert_eq!(
            report.summary.typed_path_decision_counts.get("rejected"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .typed_clone_selection_reason_counts
                .get("typed_i1_function_direct_call"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .typed_clone_rejection_reason_counts
                .get("param_not_i1"),
            Some(&1)
        );
        assert_eq!(report.evidence.typed_clone_decisions.len(), 2);
        assert_eq!(
            report.evidence.typed_clone_decisions[0]
                .reason_category
                .as_deref(),
            Some("typed_i1_function_direct_call")
        );
        assert_eq!(
            report.evidence.typed_clone_decisions[1].decision.as_deref(),
            Some("typed_clone_rejected")
        );
        assert_eq!(
            report.evidence.typed_clone_decisions[1]
                .reason_category
                .as_deref(),
            Some("param_not_i1")
        );
    }

    #[test]
    fn report_counts_typed_i32_selection_reason() {
        let artifact = json!({
            "schema_version": 14,
            "module": "typed-i32.ts",
            "records": [
                {
                    "function": "caller",
                    "source_function": "caller",
                    "expr_kind": "Call",
                    "consumer": "typed_i32_func_ref_call",
                    "native_rep_name": "js_value",
                    "native_value_state": "region_local",
                    "notes": [
                        "typed_clone=perry_fn_typed__mix__typed_i32",
                        "generic_wrapper=perry_fn_typed__mix__generic"
                    ]
                }
            ]
        });
        let report = build_report_from_artifacts(
            Path::new("/tmp/lowering"),
            vec![(PathBuf::from("native-reps.json"), artifact)],
        );

        assert_eq!(
            report
                .summary
                .typed_clone_selection_reason_counts
                .get("typed_i32_function_direct_call"),
            Some(&1)
        );
        assert_eq!(
            report.evidence.typed_clone_decisions[0]
                .reason_category
                .as_deref(),
            Some("typed_i32_function_direct_call")
        );
        assert_eq!(
            report
                .summary
                .generic_fallback_reason_counts
                .get("generic_wrapper"),
            Some(&1)
        );
    }

    #[test]
    fn report_counts_collection_helper_selection_and_rejection_reasons() {
        let artifact = json!({
            "schema_version": 14,
            "module": "collections.ts",
            "records": [
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "MapSet",
                    "consumer": "collection_string_key.map_set_string_bool",
                    "native_rep_name": "i1",
                    "native_value_state": "region_local",
                    "consumed_facts": [
                        {
                            "fact_id": "map.string_key_helper",
                            "kind": "type_fact",
                            "local_id": null,
                            "state": "consumed"
                        },
                        {
                            "fact_id": "map.boolean_value_helper",
                            "kind": "type_fact",
                            "local_id": null,
                            "state": "consumed"
                        }
                    ],
                    "notes": [
                        "selected_helper=js_map_set_string_bool",
                        "key_rep=string_ref",
                        "value_rep=i1",
                        "boxed_key_avoided=true",
                        "boxed_value_avoided_until_map_slot=true"
                    ]
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "SetAdd",
                    "consumer": "collection_typed_value.set_add_bool",
                    "native_rep_name": "i1",
                    "native_value_state": "region_local",
                    "consumed_facts": [
                        {
                            "fact_id": "set.boolean_value_helper",
                            "kind": "type_fact",
                            "local_id": null,
                            "state": "consumed"
                        }
                    ],
                    "notes": [
                        "selected_helper=js_set_add_bool",
                        "value_rep=i1",
                        "boxed_value_avoided_until_set_slot=true"
                    ]
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "SetHas",
                    "consumer": "collection_typed_value.set_has_generic",
                    "native_rep_name": "js_value",
                    "native_value_state": "region_local",
                    "rejected_facts": [
                        {
                            "fact_id": "set.boolean_value_helper",
                            "kind": "type_fact",
                            "local_id": null,
                            "state": "rejected"
                        }
                    ],
                    "notes": [
                        "generic_helper=js_set_has",
                        "typed_collection_rejected=value_expr_not_native_i1",
                        "value_rep=js_value"
                    ]
                }
            ]
        });
        let report = build_report_from_artifacts(
            Path::new("/tmp/lowering"),
            vec![(PathBuf::from("native-reps.json"), artifact)],
        );

        assert_eq!(report.summary.collection_helper_selections, 2);
        assert_eq!(report.summary.collection_helper_fallback_decisions, 1);
        assert_eq!(report.summary.collection_typed_value_selections, 2);
        assert_eq!(report.summary.collection_typed_value_fallback_decisions, 1);
        assert_eq!(report.summary.generic_fallback_emissions, 1);
        assert_eq!(
            report
                .summary
                .collection_helper_decision_counts
                .get("selected"),
            Some(&2)
        );
        assert_eq!(
            report
                .summary
                .collection_helper_decision_counts
                .get("rejected"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .collection_helper_family_counts
                .get("collection_string_key"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .collection_helper_family_counts
                .get("collection_typed_value"),
            Some(&2)
        );
        assert_eq!(
            report
                .summary
                .collection_helper_selection_reason_counts
                .get("collection_string_key.map_set_string_bool:js_map_set_string_bool"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .collection_helper_selection_reason_counts
                .get("collection_typed_value.set_add_bool:js_set_add_bool"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .collection_helper_rejection_reason_counts
                .get("value_expr_not_native_i1:js_set_has"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .generic_fallback_reason_counts
                .get("collection_typed_value.set_has_generic:js_set_has"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .collection_typed_value_decision_counts
                .get("selected"),
            Some(&2)
        );
        assert_eq!(
            report
                .summary
                .collection_typed_value_decision_counts
                .get("rejected"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .collection_typed_value_selection_reason_counts
                .get("map.boolean_value_helper:js_map_set_string_bool"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .collection_typed_value_selection_reason_counts
                .get("set.boolean_value_helper:js_set_add_bool"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .collection_typed_value_rejection_reason_counts
                .get("set.boolean_value_helper:value_expr_not_native_i1:js_set_has"),
            Some(&1)
        );
        assert_eq!(report.evidence.collection_helper_decisions.len(), 3);
        assert_eq!(report.evidence.collection_typed_value_decisions.len(), 3);
        assert_eq!(
            report.evidence.collection_helper_decisions[0]
                .decision
                .as_deref(),
            Some("collection_helper_selected")
        );
        assert_eq!(
            report.evidence.collection_helper_decisions[2]
                .decision
                .as_deref(),
            Some("collection_helper_rejected")
        );
        assert_eq!(
            report.evidence.collection_typed_value_decisions[0]
                .decision
                .as_deref(),
            Some("collection_typed_value_selected")
        );
        assert_eq!(
            report.evidence.collection_typed_value_decisions[2]
                .decision
                .as_deref(),
            Some("collection_typed_value_rejected")
        );

        let json = serde_json::to_value(&report).unwrap();
        assert!(json["summary"]["collection_helper_decision_counts"].is_object());
        assert!(json["summary"]["collection_typed_value_decision_counts"].is_object());
        assert!(json["evidence"]["collection_helper_decisions"].is_array());
        assert!(json["evidence"]["collection_typed_value_decisions"].is_array());
    }

    #[test]
    fn report_counts_field_bounds_and_scalar_evidence() {
        let artifact = json!({
            "schema_version": 14,
            "module": "fields.ts",
            "records": [
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "ClassFieldGet",
                    "consumer": "class_field_get.raw_f64_load",
                    "native_rep_name": "f64",
                    "native_value_state": "region_local",
                    "access_mode": "checked_native",
                    "bounds_state": {"proven": {"proof": "loop_guard"}},
                    "consumed_facts": [
                        {
                            "fact_id": "native_region.raw_f64_layout.1.field_x",
                            "kind": "raw_f64_layout",
                            "local_id": 1,
                            "state": "consumed"
                        }
                    ],
                    "notes": []
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "PropertyGet",
                    "consumer": "js_object_get_field_by_name",
                    "native_rep_name": "js_value",
                    "native_value_state": "materialized",
                    "materialization_reason": "runtime_api",
                    "notes": []
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "ScalarObjectFieldGet",
                    "consumer": "scalar_object_field_load.raw_f64",
                    "native_rep_name": "f64",
                    "native_value_state": "region_local",
                    "notes": ["field=x", "raw_f64_field=1"]
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "ArraySet",
                    "consumer": "numeric_array_store",
                    "native_rep_name": "f64",
                    "native_value_state": "region_local",
                    "notes": ["barrier=elided"]
                }
            ]
        });
        let report = build_report_from_artifacts(
            Path::new("/tmp/lowering"),
            vec![(PathBuf::from("native-reps.json"), artifact)],
        );

        assert_eq!(report.summary.direct_field_loads, 2);
        assert_eq!(report.summary.runtime_property_gets, 1);
        assert_eq!(report.summary.bounds_eliminations, 1);
        assert_eq!(report.summary.scalar_replacements, 1);
        assert_eq!(report.summary.boxes_inserted, 1);
        assert_eq!(report.summary.barrier_eliminations, 1);
        assert_eq!(
            report
                .summary
                .runtime_property_get_reason_counts
                .get("runtime_api"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .direct_field_load_reason_counts
                .get("raw_f64_layout:consumed"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .direct_field_load_reason_counts
                .get("scalar_replacement_raw_f64_field"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .scalar_replacement_reason_counts
                .get("scalar_replacement_raw_f64_field"),
            Some(&1)
        );
        assert_eq!(
            report.evidence.scalar_replacements[0]
                .reason_category
                .as_deref(),
            Some("scalar_replacement_raw_f64_field")
        );
        assert_eq!(
            report
                .summary
                .bounds_eliminated_reason_counts
                .get("loop_guard"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .barrier_elimination_reason_counts
                .get("barrier_elided"),
            Some(&1)
        );
    }

    #[test]
    fn report_derives_non_clone_reasons_without_explicit_reason_notes() {
        let artifact = json!({
            "schema_version": 14,
            "module": "non_clone_reasons.ts",
            "records": [
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "ScalarObjectFieldGet",
                    "consumer": "scalar_object_field_load.raw_f64",
                    "native_rep_name": "f64",
                    "native_value_state": "region_local",
                    "notes": ["field=x", "raw_f64_field=1"]
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "WriteBarrier",
                    "consumer": "write_barrier.child_bits",
                    "native_rep_name": "js_value_bits",
                    "native_value_state": "region_local",
                    "notes": []
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "IndexGet",
                    "consumer": "native_array_checked_load",
                    "native_rep_name": "f64",
                    "native_value_state": "region_local",
                    "access_mode": "checked_native",
                    "notes": []
                }
            ]
        });
        let report = build_report_from_artifacts(
            Path::new("/tmp/lowering"),
            vec![(PathBuf::from("native-reps.json"), artifact)],
        );

        assert_eq!(
            report
                .summary
                .direct_field_load_reason_counts
                .get("scalar_replacement_raw_f64_field"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .barrier_emission_reason_counts
                .get("maybe_pointer_child"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .bounds_kept_reason_counts
                .get("unknown_bounds"),
            Some(&1)
        );
        assert!(!report
            .summary
            .direct_field_load_reason_counts
            .contains_key(NOT_RECORDED));
        assert!(!report
            .summary
            .scalar_replacement_reason_counts
            .contains_key(NOT_RECORDED));
        assert!(!report
            .summary
            .barrier_emission_reason_counts
            .contains_key(NOT_RECORDED));
        assert!(!report
            .summary
            .bounds_kept_reason_counts
            .contains_key(NOT_RECORDED));
    }

    #[test]
    fn report_classifies_scalar_method_inline_and_materialized_fallback_facts() {
        let artifact = json!({
            "schema_version": 14,
            "module": "scalar_methods.ts",
            "records": [
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "ScalarMethodCall",
                    "consumer": "scalar_method_summary_inline",
                    "native_rep_name": "f64",
                    "native_value_state": "region_local",
                    "consumed_facts": [
                        {
                            "fact_id": "native_region.scalar_method_summary.1.Point.len",
                            "kind": "scalar_method_summary",
                            "local_id": 1,
                            "state": "consumed",
                            "detail": "exact_receiver_summary"
                        }
                    ],
                    "notes": [
                        "class=Point",
                        "method=len",
                        "receiver=scalar_replaced",
                        "arg_proof=proven_numeric"
                    ]
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "ScalarMethodCall",
                    "consumer": "scalar_method_summary_materialized_fallback",
                    "native_rep_name": "js_value",
                    "native_value_state": "materialized",
                    "access_mode": "dynamic_fallback",
                    "materialization_reason": "runtime_api",
                    "rejected_facts": [
                        {
                            "fact_id": "native_region.scalar_method_summary.1.Point.len",
                            "kind": "scalar_method_summary",
                            "local_id": 1,
                            "state": "arg_guard_failed",
                            "detail": "guarded_numeric_args_fallback"
                        }
                    ],
                    "notes": [
                        "class=Point",
                        "method=len",
                        "receiver=scalar_replaced",
                        "scalar_method_fallback=arg_guard_failed",
                        "arg_guard=js_typed_f64_arg_guard"
                    ]
                },
                {
                    "function": "probe",
                    "source_function": "probe",
                    "expr_kind": "ScalarMethodCall",
                    "consumer": "scalar_method_summary_materialized_fallback",
                    "native_rep_name": "js_value",
                    "native_value_state": "materialized",
                    "access_mode": "dynamic_fallback",
                    "materialization_reason": "runtime_api",
                    "rejected_facts": [
                        {
                            "fact_id": "native_region.scalar_method_summary.1.Point.len",
                            "kind": "scalar_method_summary",
                            "local_id": 1,
                            "state": "generic_arg",
                            "detail": "generic_argument"
                        }
                    ],
                    "notes": [
                        "class=Point",
                        "method=len",
                        "receiver=scalar_replaced",
                        "scalar_method_fallback=generic_arg"
                    ]
                }
            ]
        });
        let report = build_report_from_artifacts(
            Path::new("/tmp/lowering"),
            vec![(PathBuf::from("native-reps.json"), artifact)],
        );

        assert_eq!(report.summary.scalar_replacements, 1);
        assert_eq!(report.summary.scalar_replacement_fallbacks, 2);
        assert_eq!(report.summary.scalar_replacement_rejections, 0);
        assert_eq!(
            report
                .summary
                .scalar_replacement_decision_counts
                .get("selected"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .scalar_replacement_decision_counts
                .get("fallback"),
            Some(&2)
        );
        assert_eq!(
            report
                .summary
                .scalar_replacement_reason_counts
                .get("scalar_method_summary:exact_receiver_summary"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .scalar_replacement_reason_counts
                .get("scalar_method_materialized_fallback:guarded_numeric_args_fallback"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .scalar_replacement_reason_counts
                .get("scalar_method_materialized_fallback:generic_argument"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .scalar_replacement_selection_reason_counts
                .get("scalar_method_summary:exact_receiver_summary"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .scalar_replacement_fallback_reason_counts
                .get("scalar_method_materialized_fallback:guarded_numeric_args_fallback"),
            Some(&1)
        );
        assert_eq!(
            report
                .summary
                .scalar_replacement_fallback_reason_counts
                .get("scalar_method_materialized_fallback:generic_argument"),
            Some(&1)
        );
        assert_eq!(
            report.evidence.scalar_replacements[0].decision.as_deref(),
            Some("scalar_replacement_selected")
        );
        assert_eq!(
            report.evidence.scalar_replacements[0]
                .reason_category
                .as_deref(),
            Some("scalar_method_summary:exact_receiver_summary")
        );
        assert_eq!(
            report.evidence.scalar_replacements[1].decision.as_deref(),
            Some("scalar_replacement_fallback")
        );
        assert_eq!(
            report.evidence.scalar_replacements[1]
                .reason_category
                .as_deref(),
            Some("scalar_method_materialized_fallback:guarded_numeric_args_fallback")
        );
        assert_eq!(
            report.evidence.scalar_replacements[2].decision.as_deref(),
            Some("scalar_replacement_fallback")
        );
        assert_eq!(
            report.evidence.scalar_replacements[2]
                .reason_category
                .as_deref(),
            Some("scalar_method_materialized_fallback:generic_argument")
        );
    }
}
