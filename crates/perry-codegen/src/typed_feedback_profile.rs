//! Versioned, advisory typed-feedback replay. No observation is a runtime proof.
//!
//! The driver supplies source/compiler/configuration identity; this module matches
//! exact sites during lowering. State is scoped to one synchronous codegen call on
//! a rayon worker, and restored on every exit (including errors and unwinding).
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::native_value::{NativeFactUse, NativeRepRecord};
use crate::{compile_module, CompileOptions};

pub fn effective_target(opts: &CompileOptions) -> String {
    opts.target
        .clone()
        .unwrap_or_else(crate::codegen::helpers::default_target_triple)
}

pub const SCHEMA_VERSION: u32 = 1;
pub const NUMERIC_ARRAY_ELEMENT: &str = "numeric_array_element";
pub(crate) const NUMERIC_GUARD: &str = "numeric_array_index_get_guard";
pub(crate) const ARRAY_FALLBACK: &str = "js_typed_feedback_array_index_get_fallback_boxed";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub schema_version: u32,
    /// Exact compiler executable SHA-256, including same-version development builds.
    pub compiler: String,
    pub modules: Vec<ModuleProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleIdentity {
    pub module: String,
    pub source_hash: String,
    pub hir_hash: String,
    pub lowering_hash: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleProfile {
    pub identity: ModuleIdentity,
    pub sites: Vec<Site>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    pub site_id: u64,
    pub function: String,
    pub kind: String,
    pub operation: String,
    /// Only numeric_array_element is currently supported. Captured catalogs use
    /// unobserved until joined with a runtime trace by the capture utility.
    pub observation_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Decision {
    pub module: String,
    pub site_id: Option<u64>,
    pub function: String,
    pub accepted: bool,
    pub reason: String,
}

pub struct Session {
    compiler: String,
    profile: Option<Profile>,
    captured: Mutex<BTreeMap<String, ModuleProfile>>,
    decisions: Mutex<Vec<Decision>>,
}

impl Session {
    pub fn new(compiler: String, profile: Option<Profile>) -> Self {
        Self {
            compiler,
            profile,
            captured: Mutex::new(BTreeMap::new()),
            decisions: Mutex::new(Vec::new()),
        }
    }

    /// Strict parsing for explicit input; unknown schema/observation versions are
    /// well-formed but rejected later, with an explanation for every entry.
    pub fn read_profile(path: &Path) -> Result<Profile> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("cannot read --typed-feedback-profile {}", path.display()))?;
        let diagnostic = || {
            format!("invalid --typed-feedback-profile {}: expected a versioned replay profile; create one with scripts/typed-feedback-profile.py", path.display())
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes).with_context(diagnostic)?;
        // A future schema may use a different body. Its version is sufficient
        // to reject the whole profile without interpreting unknown fields.
        if let Some(version) = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v != SCHEMA_VERSION)
        {
            return Ok(serde_json::from_value(value).unwrap_or_else(|_| Profile {
                schema_version: version,
                compiler: String::new(),
                modules: Vec::new(),
            }));
        }
        serde_json::from_value(value).with_context(diagnostic)
    }

    pub fn compile_module(
        &self,
        hir: &perry_hir::Module,
        opts: CompileOptions,
        identity: ModuleIdentity,
    ) -> Result<Vec<u8>> {
        let state = ModuleState::new(&self.compiler, self.profile.as_ref(), identity);
        let previous = ACTIVE.with(|active| active.replace(Some(state)));
        let _scope = Scope(previous);
        let result = compile_module(hir, opts);
        let state = ACTIVE
            .with(|active| active.borrow_mut().take())
            .expect("feedback scope");
        if result.is_ok() {
            self.decisions.lock().unwrap().extend(state.decisions);
            self.captured.lock().unwrap().insert(
                state.identity.module.clone(),
                ModuleProfile {
                    identity: state.identity,
                    sites: state.sites.into_values().collect(),
                },
            );
        }
        result
    }

    /// Call after all modules finish, before explain-lowering reads artifacts.
    pub fn finish(&self, catalog_path: Option<&Path>) -> Result<Vec<Decision>> {
        let captured = self.captured.lock().unwrap();
        if let Some(path) = catalog_path {
            let catalog = Profile {
                schema_version: SCHEMA_VERSION,
                compiler: self.compiler.clone(),
                modules: captured.values().cloned().collect(),
            };
            std::fs::write(
                path,
                format!("{}\n", serde_json::to_string_pretty(&catalog)?),
            )
            .with_context(|| {
                format!(
                    "cannot write typed-feedback site catalog {}",
                    path.display()
                )
            })?;
        }
        let mut decisions = self.decisions.lock().unwrap().clone();
        let mut unmatched = Vec::new();
        if let Some(profile) = &self.profile {
            // Even an empty incompatible profile needs a profile-level diagnostic.
            if let Some(reason) = profile_rejection(&self.compiler, profile) {
                let decision = Decision {
                    module: "<profile>".into(),
                    site_id: None,
                    function: String::new(),
                    accepted: false,
                    reason: reason.into(),
                };
                unmatched.push(rejection_record(&decision));
                decisions.push(decision);
            }
            for module in &profile.modules {
                if !captured.contains_key(&module.identity.module) {
                    let reason =
                        profile_rejection(&self.compiler, profile).unwrap_or("unknown_module");
                    if module.sites.is_empty() {
                        let decision = module_rejection(&module.identity.module, reason);
                        unmatched.push(rejection_record(&decision));
                        decisions.push(decision);
                    }
                    for site in &module.sites {
                        let decision = rejected(&module.identity.module, site, reason);
                        unmatched.push(rejection_record(&decision));
                        decisions.push(decision);
                    }
                }
            }
        }
        if !unmatched.is_empty() {
            crate::native_value::write_native_rep_artifact_if_enabled(
                "typed_feedback_profile",
                &unmatched,
            )?;
        }
        decisions.sort();
        Ok(decisions)
    }
}

fn profile_rejection(compiler: &str, profile: &Profile) -> Option<&'static str> {
    if profile.schema_version != SCHEMA_VERSION {
        Some("schema_mismatch")
    } else if profile.compiler != compiler {
        Some("compiler_mismatch")
    } else {
        None
    }
}

fn identity_rejection(expected: &ModuleIdentity, actual: &ModuleIdentity) -> Option<&'static str> {
    if expected.source_hash != actual.source_hash {
        Some("source_hash_mismatch")
    } else if expected.target != actual.target {
        Some("target_mismatch")
    } else if expected.hir_hash != actual.hir_hash {
        Some("hir_hash_mismatch")
    } else if expected.lowering_hash != actual.lowering_hash {
        Some("lowering_inputs_mismatch")
    } else {
        None
    }
}

struct ModuleState {
    compiler: String,
    identity: ModuleIdentity,
    sites: BTreeMap<u64, Site>,
    pending: BTreeMap<u64, Site>,
    decisions: Vec<Decision>,
}

impl ModuleState {
    fn new(compiler: &str, profile: Option<&Profile>, identity: ModuleIdentity) -> Self {
        let mut state = Self {
            compiler: compiler.into(),
            identity,
            sites: BTreeMap::new(),
            pending: BTreeMap::new(),
            decisions: Vec::new(),
        };
        if let Some(profile) = profile {
            let modules: Vec<_> = profile
                .modules
                .iter()
                .filter(|m| m.identity.module == state.identity.module)
                .collect();
            for module in &modules {
                let reason = profile_rejection(compiler, profile)
                    .or_else(|| (modules.len() != 1).then_some("duplicate_module"))
                    .or_else(|| identity_rejection(&module.identity, &state.identity));
                if module.sites.is_empty() {
                    if let Some(reason) = reason {
                        state
                            .decisions
                            .push(module_rejection(&state.identity.module, reason));
                    }
                }
                let mut seen = BTreeSet::new();
                let duplicates: BTreeSet<_> = module
                    .sites
                    .iter()
                    .filter_map(|s| (!seen.insert(s.site_id)).then_some(s.site_id))
                    .collect();
                for site in &module.sites {
                    let reason = reason
                        .or_else(|| {
                            duplicates
                                .contains(&site.site_id)
                                .then_some("duplicate_site")
                        })
                        .or_else(|| {
                            (site.observation_kind != NUMERIC_ARRAY_ELEMENT)
                                .then_some("unsupported_observation_kind")
                        });
                    if let Some(reason) = reason {
                        state
                            .decisions
                            .push(rejected(&state.identity.module, site, reason));
                    } else {
                        state.pending.insert(site.site_id, site.clone());
                    }
                }
            }
        }
        state
    }
}

thread_local! {
    static ACTIVE: RefCell<Option<ModuleState>> = const { RefCell::new(None) };
}
struct Scope(Option<ModuleState>);
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.replace(self.0.take());
        });
    }
}

pub(crate) fn register_site(site_id: u64, function: &str, kind: &str, operation: &str) {
    ACTIVE.with(|active| {
        if let Some(state) = active.borrow_mut().as_mut() {
            state.sites.insert(
                site_id,
                Site {
                    site_id,
                    function: function.into(),
                    kind: kind.into(),
                    operation: operation.into(),
                    observation_kind: "unobserved".into(),
                },
            );
        }
    });
}

/// The sole selection seam: called only for an existing plain, checked array
/// read. The caller must emit the full numeric guard (inline or runtime) and
/// boxed fallback.
pub(crate) fn select_numeric_array(site_id: u64, already_numeric: bool) -> Option<NativeFactUse> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let state = active.as_mut()?;
        let observed = state.pending.remove(&site_id)?;
        let site = state.sites.get(&site_id)?;
        let reason = if observed.function != site.function || observed.kind != site.kind || observed.operation != site.operation {
            Some("site_identity_mismatch")
        } else if already_numeric {
            Some("already_specialized")
        } else {
            None
        };
        if let Some(reason) = reason {
            state.decisions.push(rejected(&state.identity.module, &observed, reason));
            return None;
        }
        state.decisions.push(Decision { module: state.identity.module.clone(), site_id: Some(site_id), function: site.function.clone(), accepted: true, reason: "fresh_numeric_array_observation".into() });
        Some(NativeFactUse {
            fact_id: format!("typed_feedback_replay:{}:{site_id}", state.identity.module),
            kind: "typed_feedback_replay".into(), local_id: None, state: "consumed".into(),
            detail: format!("fresh_numeric_array_observation;schema_version={};compiler={};source_hash={};hir_hash={};lowering_hash={};target={};advisory=true", SCHEMA_VERSION, state.compiler, state.identity.source_hash, state.identity.hir_hash, state.identity.lowering_hash, state.identity.target),
            reason: None,
        })
    })
}

pub(crate) fn finish_module(records: &mut Vec<NativeRepRecord>) {
    ACTIVE.with(|active| {
        if let Some(state) = active.borrow_mut().as_mut() {
            for (id, site) in std::mem::take(&mut state.pending) {
                let reason = if state.sites.contains_key(&id) {
                    "unsupported_site"
                } else {
                    "unknown_site"
                };
                state
                    .decisions
                    .push(rejected(&state.identity.module, &site, reason));
            }
            state.decisions.sort();
            records.extend(
                state
                    .decisions
                    .iter()
                    .filter(|d| !d.accepted)
                    .map(rejection_record),
            );
        }
    });
}

fn module_rejection(module: &str, reason: &str) -> Decision {
    Decision {
        module: module.into(),
        site_id: None,
        function: String::new(),
        accepted: false,
        reason: reason.into(),
    }
}

fn rejected(module: &str, site: &Site, reason: &str) -> Decision {
    Decision {
        module: module.into(),
        site_id: Some(site.site_id),
        function: site.function.clone(),
        accepted: false,
        reason: reason.into(),
    }
}

fn rejection_record(decision: &Decision) -> NativeRepRecord {
    // Reuse the ordinary decision-record representation, with replay's own
    // discriminator and facts (not a typed-clone decision).
    let mut record = crate::native_value::typed_clone_rejection_record(
        &decision.function,
        "typed_feedback_profile",
        &decision.reason,
        Vec::new(),
    );
    record.expr_kind = "TypedFeedbackReplayDecision".into();
    record.notes = vec![
        format!("typed_feedback_replay_rejected={}", decision.reason),
        format!("profile_module={}", decision.module),
    ];
    record.rejected_facts.push(NativeFactUse {
        fact_id: format!(
            "typed_feedback_replay:{}:{}",
            decision.module,
            decision
                .site_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "profile".into())
        ),
        kind: "typed_feedback_replay".into(),
        local_id: None,
        state: "rejected".into(),
        detail: decision.reason.clone(),
        reason: None,
    });
    record
}

#[cfg(test)]
mod tests;

/// Replay claims are valid only when tied to a consumed, fresh observation,
/// the runtime numeric-layout/bounds proof, and the emitted boxed side exit.
pub(crate) fn verify_records(records: &[NativeRepRecord], errors: &mut Vec<String>) {
    use crate::native_value::{BoundsState, BufferAccessMode, MaterializationReason};
    for record in records {
        let claims_selection = record
            .notes
            .iter()
            .any(|n| n.starts_with("typed_feedback_replay_selected="));
        let facts: Vec<_> = record
            .consumed_facts
            .iter()
            .filter(|f| f.kind == "typed_feedback_replay")
            .collect();
        if !claims_selection && facts.is_empty() {
            continue;
        }
        let valid_fact = claims_selection
            && facts.len() == 1
            && facts[0].state == "consumed"
            && facts[0]
                .detail
                .starts_with("fresh_numeric_array_observation;");
        let valid_guard = record.expr_kind == "NumericArrayIndexGet"
            && record.consumer == "js_array_numeric_get_f64_unboxed"
            && record.native_rep == crate::native_value::NativeRep::F64
            && matches!(&record.bounds_state, Some(BoundsState::Guarded { guard_id }) if guard_id == NUMERIC_GUARD)
            && record.access_mode == Some(BufferAccessMode::CheckedNative)
            && record.consumed_facts.iter().any(|f| {
                f.kind == "raw_f64_layout" && f.state == "consumed" && f.detail == NUMERIC_GUARD
            });
        let valid_fallback = valid_fact
            && records.iter().any(|fallback| {
                fallback.function == record.function
                    && fallback.block_label != record.block_label
                    && fallback.consumer == ARRAY_FALLBACK
                    && fallback.access_mode == Some(BufferAccessMode::DynamicFallback)
                    && fallback.materialization_reason == Some(MaterializationReason::RuntimeApi)
                    && fallback.notes.contains(&format!(
                        "typed_feedback_replay_fallback={}",
                        facts[0].fact_id
                    ))
            });
        if !valid_fact || !valid_guard || !valid_fallback {
            errors.push(format!("{}:{} profile-directed specialization requires a consumed fresh replay fact, matching runtime guard, and explicit fallback/materialization record", record.function, record.block_label));
        }
    }
}
