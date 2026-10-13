//! wpfacts (whole-program invariance facts) slice 1 in the driver: run the
//! analysis once over the final HIR of every module, write the facts dump,
//! and hand each module's codegen its view (digest + contract claims).
//!
//! Knobs (all keyed into the object cache through the view):
//! - `PERRY_PROGRAM_FACTS=0` turns the pass off (every fact off, digest 0);
//! - `PERRY_FACTS_DUMP=<path>` writes the facts dump (and `<path>.rel`, the
//!   constraint relations for `scripts/wpfacts_recompute.py`, when
//!   `PERRY_FACTS_RELATIONS=1`);
//! - `PERRY_FACTS_SABOTAGE=<spec>` plants one wrong fact (negative control);
//! - `PERRY_CHECK_CONTRACTS=1` makes this a contract build: each module
//!   carries its claims in a read-only `.perry_facts` section and the
//!   runtime (feature `contract-checks`) checks every dynamic operation
//!   against them;
//! - `PERRY_FACTS_REPORT=1` prints the pass statistics and wall time;
//! - `PERRY_FACTS_MAX_WORK` (default `DEFAULT_MAX_WORK`): a deterministic
//!   work bound on the solve (token propagations); past it the solve stops
//!   and the program gets no facts at all (the sound fallback). 0 = none;
//! - `PERRY_FACTS_BUDGET_SECS` (default off): an optional wall-clock bound
//!   with the same fallback (machine-dependent, so never the default);
//! - `PERRY_FACTS_PROFILE=1`: per-constraint-kind solve profile in the dump;
//! - `PERRY_FACTS_EXPLAIN=1`: diagnostics — each ⊤ site verdict names
//!   where ⊤ first entered (the precision report groups by it);
//! - `PERRY_FACTS_ONLY=1`: stop after writing the dump (analysis-only runs);
//! - `PERRY_FACTS_WHATIF_NO_E14=1`: REPORT ONLY, unsound — ignore the
//!   program-wide E14 kill to measure what it costs (the dump says so).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Solve-work bound (see `perry_wpfacts::FactsConfig::max_work`), sized so
/// the whole pass costs a few seconds at most. Programs the analysis cannot
/// finish within it (tsc, Effect, fastify) get no facts anyway. 16M is about
/// 2 s of solve on the build host; zod needs 6.2M, commander 2.0M, the
/// class-local kernels under 400.
const DEFAULT_MAX_WORK: u64 = 16_000_000;

pub(crate) fn contracts_requested() -> bool {
    std::env::var("PERRY_CHECK_CONTRACTS")
        .is_ok_and(|v| matches!(v.trim(), "1" | "true" | "on" | "yes"))
}

pub(crate) fn run(
    modules: &BTreeMap<PathBuf, perry_hir::Module>,
    dynamic_code: bool,
    wrapped: &std::collections::HashMap<PathBuf, super::types::CjsWrapDebugSource>,
) -> Option<Arc<perry_wpfacts::ProgramFacts>> {
    if !perry_wpfacts::enabled_from_env() {
        return None;
    }
    let started = std::time::Instant::now();
    let dump = std::env::var_os("PERRY_FACTS_DUMP").map(PathBuf::from);
    let relations =
        dump.is_some() && std::env::var("PERRY_FACTS_RELATIONS").is_ok_and(|v| v == "1");
    let cfg = perry_wpfacts::FactsConfig {
        relations,
        sabotage: std::env::var("PERRY_FACTS_SABOTAGE")
            .ok()
            .filter(|s| !s.is_empty()),
        // Past either bound the program simply gets no facts (sound).
        budget_secs: std::env::var("PERRY_FACTS_BUDGET_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|&s| s != 0),
        max_work: Some(
            std::env::var("PERRY_FACTS_MAX_WORK")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_MAX_WORK),
        ),
        whatif_no_e14: std::env::var("PERRY_FACTS_WHATIF_NO_E14").is_ok_and(|v| v == "1"),
        profile: std::env::var("PERRY_FACTS_PROFILE").is_ok_and(|v| v == "1"),
        // Widening: a points-to set past this many tokens becomes ⊤ and its
        // members escape (sound; bounds memory on merged generic helpers).
        widen_k: Some(
            std::env::var("PERRY_FACTS_WIDEN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(64),
        ),
        // Points-to entries bound (memory); past it the program gets no facts.
        max_pts: Some(
            std::env::var("PERRY_FACTS_MAX_PTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(400_000_000),
        ),
        explain: std::env::var("PERRY_FACTS_EXPLAIN").is_ok_and(|v| v == "1"),
        dynamic_code: dynamic_code.then(|| {
            "E14 the program can run interpreted code (deferred new Function/eval or node:vm)"
                .into()
        }),
    };
    let list: Vec<(String, &perry_hir::Module)> = modules
        .iter()
        .map(|(p, m)| (p.display().to_string(), m))
        .collect();
    let pf = perry_wpfacts::compute(&list, &cfg);
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    if let Some(path) = &dump {
        let mut text = pf.dump_text();
        text.push_str(&format!("# pass_wall_ms {elapsed:.1}\n"));
        // CJS modules are parsed wrapped: their site offsets are in wrapped
        // coordinates. With source locations captured (`--debug-symbols`),
        // keep each wrapped text beside the dump so reports can map a site
        // back to its original line (`W <module> <prefix lines> <file>`).
        if !wrapped.is_empty() {
            let mut dir = path.clone().into_os_string();
            dir.push(".src");
            let dir = PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&dir);
            for (i, p) in pf.module_paths.iter().enumerate() {
                if let Some(w) = wrapped.get(std::path::Path::new(p)) {
                    let f = dir.join(format!("{i}.js"));
                    if std::fs::write(&f, &w.wrapped_source).is_ok() {
                        text.push_str(&format!("W {i} {} {}\n", w.prefix_line_count, f.display()));
                    }
                }
            }
        }
        if let Err(e) = std::fs::write(path, text) {
            eprintln!("perry: wpfacts: cannot write {}: {e}", path.display());
        }
        if let Some(rel) = pf.relations_text() {
            let mut rp = path.clone().into_os_string();
            rp.push(".rel");
            if let Err(e) = std::fs::write(&rp, rel) {
                eprintln!("perry: wpfacts: cannot write relations: {e}");
            }
        }
    }
    if std::env::var("PERRY_FACTS_REPORT").is_ok_and(|v| v == "1") {
        let s = &pf.stats;
        let t0 = pf
            .sites
            .iter()
            .filter(|s| !matches!(s.verdict, perry_wpfacts::Verdict::No(_)))
            .count();
        eprintln!(
            "perry: wpfacts: {} modules, {} templates ({} fact units), {} nodes, {} constraints; \
             {} of {} read/call sites proven; gen {:.0} ms, solve {:.0} ms ({} pops, work {}{}), derive {:.0} ms, wall {:.0} ms",
            s.modules,
            s.templates,
            s.fact_units,
            s.nodes,
            s.constraints,
            t0,
            pf.sites.len(),
            s.gen_ms,
            s.solve_ms,
            s.pops,
            s.work,
            if s.aborted { ", OVER the work/time bound: no facts" } else { "" },
            s.derive_ms,
            elapsed
        );
    }
    if std::env::var("PERRY_FACTS_ONLY").is_ok_and(|v| v == "1") {
        // Analysis-only run (precision reports, checkers): stop after the
        // dump, before codegen.
        std::process::exit(0);
    }
    Some(Arc::new(pf))
}

/// Whether any class is prefix-stable with its facts usable for elision
/// (step 5b: the only classes a typed body's receiver proof can use).
pub(crate) fn has_stable_classes(pf: &Option<Arc<perry_wpfacts::ProgramFacts>>) -> bool {
    pf.as_ref().is_some_and(|pf| {
        elide_enabled()
            && pf.global.is_none()
            && !pf.stats.aborted
            && !pf.whatif_no_e14
            && pf
                .templates
                .iter()
                .any(|t| t.prefix_stable && t.kind == "class" && t.class_id.is_some())
    })
}

/// The view module `path`'s codegen receives.
pub(crate) fn view_for(
    pf: &Option<Arc<perry_wpfacts::ProgramFacts>>,
    path: &Path,
    receiver_params: Option<&BTreeMap<u32, Vec<String>>>,
) -> perry_codegen::ProgramFactsView {
    let Some(pf) = pf else {
        return Default::default();
    };
    if pf.whatif_no_e14 {
        // A report-only what-if run never reaches codegen.
        return Default::default();
    }
    let key = path.display().to_string();
    let section = if contracts_requested() {
        pf.contract_section(&key)
    } else {
        Vec::new()
    };
    let mut elision = elision_for(pf, &key);
    if !elision.classes.is_empty() {
        if let Some(r) = receiver_params {
            elision.receiver_params = r.clone();
        }
    }
    let digest = if elision.is_empty() {
        pf.digest_for(&key)
    } else {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pf.digest_for(&key).hash(&mut h);
        elision.hash(&mut h);
        h.finish()
    };
    perry_codegen::ProgramFactsView {
        digest,
        contract_section: Arc::new(section),
        elision: Arc::new(elision),
    }
}

/// `PERRY_FACTS_ELIDE=0` keeps every check (facts still computed, dumped and
/// claimed): the A/B control for slice 2.
fn elide_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_ELIDE").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Slice 2: the receiver facts of `path`'s bindings that the zero-guard tier
/// can act on, and the layout facts of the classes they can hold.
///
/// A binding gets an entry only when every value it can hold is an instance
/// of a prefix-stable class (F3, F4), or null/undefined (`nullable`: the
/// site keeps its receiver test). ⊤, literals, builtins, functions and any
/// other primitive give no entry. A program-wide kill (E14) or an over-budget
/// solve already left no template prefix-stable.
fn elision_for(pf: &perry_wpfacts::ProgramFacts, path: &str) -> perry_codegen::ElisionFacts {
    use perry_codegen::{ElisionFacts, FactsClass, FactsField, FactsRecv};
    use perry_wpfacts::model::prim;
    let mut out = ElisionFacts {
        contracts: contracts_requested(),
        ..Default::default()
    };
    if !elide_enabled() || pf.global.is_some() || pf.stats.aborted {
        return out;
    }
    let Some(mb) = pf.bindings_for(path) else {
        return out;
    };
    let stable: std::collections::HashMap<u32, &perry_wpfacts::TemplateFacts> = pf
        .templates
        .iter()
        .filter(|t| t.prefix_stable && t.kind == "class")
        .filter_map(|t| Some((t.class_id?, t)))
        .collect();
    let recv = |b: &perry_wpfacts::BindingFacts| -> Option<FactsRecv> {
        if b.other || b.class_ids.is_empty() || b.prims & !(prim::NULL | prim::UNDEF) != 0 {
            return None;
        }
        if !b.class_ids.iter().all(|c| stable.contains_key(c)) {
            return None;
        }
        Some(FactsRecv {
            class_ids: b.class_ids.clone(),
            nullable: b.prims != 0,
        })
    };
    for (id, b) in &mb.locals {
        if let Some(r) = recv(b) {
            out.locals.insert(*id, r);
        }
    }
    for (id, b) in &mb.this_of {
        if let Some(r) = recv(b) {
            out.this_of.insert(*id, r);
        }
    }
    // Step 5b: a typed body's entry check proves a class set for a
    // parameter no binding fact covers, so every prefix-stable class's
    // layout facts are visible, not only those of classes some binding holds.
    for (&cid, &t) in &stable {
        let fields = t
            .fields
            .iter()
            .filter(|f| f.definite && f.data_field)
            .filter_map(|f| {
                Some(FactsField {
                    key: f.key.clone(),
                    slot: f.slot?,
                    readonly: f.readonly,
                    kind_mask: f.kind.mask(),
                    ta_stable: f.ta_stable,
                    ta_kind: f.ta_kind,
                })
            })
            .collect();
        out.classes.insert(
            cid,
            FactsClass {
                display: t.display.clone(),
                fixed_layout: t.fixed_layout,
                fields,
            },
        );
    }
    out
}
