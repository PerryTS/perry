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
//! - `PERRY_FACTS_UNITS=0`: whole-program facts, the reference mode (one
//!   solve over every module). The default is per-unit facts (decision 151):
//!   one solve per module, the other modules unknown code
//!   (`perry_wpfacts::compute_units`), the bounds per unit. Per-unit is the
//!   default because whole-program exact points-to does not scale (fastify
//!   does not converge); owner decision 2026-10-23.
//!
//! Interpreted code (`new Function`, `eval`, `vm`) is no program-wide kill
//! (decision 150): the analysis models it as the ⊤ summary node, and when it
//! can run every proof is guarded by the runtime's interpreted-prototype-write
//! word (`ElisionFacts::interp_guard`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Solve-work bound (see `perry_wpfacts::FactsConfig::max_work`), sized so
/// the whole pass costs a few seconds at most. With `this` bound per
/// (receiver, key), shared accessor walks and groups, tsc needed 45M, fastify 59M, Effect 27M,
/// zod 3.9M, commander 1.8M; the Next.js server (~3G) still stops at the
/// bound and gets no facts.
const DEFAULT_MAX_WORK: u64 = 200_000_000;

pub(crate) fn contracts_requested() -> bool {
    std::env::var("PERRY_CHECK_CONTRACTS")
        .is_ok_and(|v| matches!(v.trim(), "1" | "true" | "on" | "yes"))
}

/// Every module's `export … from "src"` sources resolved the way module
/// collection and init order resolve them, for the targets that are program
/// modules. The HIR keeps a re-export's source as written; without this the
/// analysis cannot link `export * as X from "./x"` to `./x`'s namespace.
pub(crate) fn export_sources(ctx: &super::CompilationContext) -> perry_wpfacts::ExportSources {
    let mut out = perry_wpfacts::ExportSources::new();
    for (path, module) in &ctx.native_modules {
        for export in &module.exports {
            let source = match export {
                perry_hir::Export::ExportAll { source }
                | perry_hir::Export::ReExport { source, .. }
                | perry_hir::Export::NamespaceReExport { source, .. } => source,
                perry_hir::Export::Named { .. } => continue,
            };
            let Some((target, _)) = super::resolve::resolve_import_with_context(source, path, ctx)
            else {
                continue;
            };
            if ctx.native_modules.contains_key(&target) {
                out.entry(path.display().to_string())
                    .or_default()
                    .insert(source.clone(), target.display().to_string());
            }
        }
    }
    out
}

pub(crate) fn run(
    modules: &BTreeMap<PathBuf, perry_hir::Module>,
    export_sources: perry_wpfacts::ExportSources,
    dynamic_code: bool,
    dynamic_code_reach: &perry_hir::dynamic_code::DynamicCodeReach,
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
        profile: std::env::var("PERRY_FACTS_PROFILE").is_ok_and(|v| v == "1"),
        // Points-to entries bound (memory); past it the program gets no facts.
        max_pts: Some(
            std::env::var("PERRY_FACTS_MAX_PTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(400_000_000),
        ),
        export_sources,
        explain: std::env::var("PERRY_FACTS_EXPLAIN").is_ok_and(|v| v == "1")
            || perry_codegen::fallback_diag::enabled(),
        dynamic_code_reach: dynamic_code_reach.clone(),
        dynamic_code: dynamic_code.then(|| {
            "E14 the program can run interpreted code (deferred new Function/eval or node:vm)"
                .into()
        }),
        unit: false,
        // Decision 32: the one unit-jobs setting (`PERRY_CODEGEN_UNIT_JOBS`,
        // else the budget's CPU slots, else cores bounded by memory).
        jobs: perry_codegen::workers::unit_workers(),
        unit_io: dump.is_some(),
        report: std::env::var("PERRY_FACTS_REPORT").is_ok_and(|v| v == "1"),
    };
    // Entry outlining moves module bindings into compiler-owned functions.
    // Facts need the same logical module scope that codegen's globals and
    // export getters use; retain the emitted HIR unchanged for codegen.
    let inputs = facts_inputs(modules);
    let list: Vec<(String, &perry_hir::Module)> = inputs
        .iter()
        .map(|(p, m)| (p.clone(), m.as_ref()))
        .collect();
    let pf = if per_unit_facts(std::env::var("PERRY_FACTS_UNITS").ok().as_deref()) {
        perry_wpfacts::compute_units(&list, &cfg)
    } else {
        perry_wpfacts::compute(&list, &cfg)
    };
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
        eprintln!(
            "perry: wpfacts: Function.prototype.bind {}",
            pf.fnproto_bind_written
                .as_deref()
                .map_or("unwritten".to_string(), |r| format!("may be written: {r}"))
        );
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
            if s.aborted {
                ", OVER the work/time bound: no facts"
            } else {
                ""
            },
            s.derive_ms,
            elapsed
        );
        for (path, u) in &pf.units {
            eprintln!(
                "perry: wpfacts: unit {path}: {} nodes, {} pts entries, work {}, {:.0} ms{}",
                u.nodes,
                u.pts_entries,
                u.work,
                u.gen_ms + u.solve_ms + u.derive_ms,
                if u.aborted {
                    ", OVER the bound: no facts"
                } else {
                    ""
                }
            );
        }
    }
    if std::env::var("PERRY_FACTS_ONLY").is_ok_and(|v| v == "1") {
        // Analysis-only run (precision reports, checkers): stop after the
        // dump, before codegen.
        std::process::exit(0);
    }
    Some(Arc::new(pf))
}

/// The view module `path`'s codegen receives.
pub(crate) fn view_for(
    pf: &Option<Arc<perry_wpfacts::ProgramFacts>>,
    path: &Path,
) -> perry_codegen::ProgramFactsView {
    let Some(pf) = pf else {
        return Default::default();
    };
    let key = path.display().to_string();
    let section = if contracts_requested() {
        pf.contract_section(&key)
    } else {
        Vec::new()
    };
    let elision = elision_for(pf, &key);
    let digest = if elision.is_empty()
        && !elision.fnproto_bind_unwritten
        && elision.super_unrelinked.is_empty()
    {
        pf.digest_for(&key)
    } else {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pf.digest_for(&key).hash(&mut h);
        elision.hash(&mut h);
        h.finish()
    };
    perry_codegen::ProgramFactsView {
        fallback: super::fallback_diag::facts_for(pf, &key),
        digest,
        dynamic_code_reach: pf.dynamic_code_reach.clone(),
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
/// A binding gets an entry when it can hold an instance of a prefix-stable
/// class, a user class or an object-literal class (F3, F4). When it holds
/// nothing else but null/undefined (`nullable`: the site keeps its receiver
/// test), the proof is unchecked (§3.7 case 1/2). When it can also hold ⊤
/// (interpreted code, unmodeled code), a literal, a builtin, a function, any
/// other primitive or an instance of a class that is not prefix-stable, it is
/// CHECKED (`checked`): the site compares the receiver's class id against
/// the prefix-stable classes and takes its complete generic operation
/// otherwise. An over-budget solve gives no entry at all.
fn elision_for(pf: &perry_wpfacts::ProgramFacts, path: &str) -> perry_codegen::ElisionFacts {
    use perry_codegen::{ElisionFacts, FactsBody, FactsClass, FactsField, FactsRecv};
    use perry_wpfacts::model::prim;
    let mut out = ElisionFacts {
        contracts: contracts_requested(),
        ..Default::default()
    };
    if !elide_enabled() || pf.stats.aborted {
        return out;
    }
    // `PERRY_FACTS_SABOTAGE=unchecked`: the negative control of decision 150
    // (every guarded or checked proof is emitted unchecked; the dynamic-code
    // gap tests must then fail).
    let unchecked = pf.sabotage.as_deref() == Some("unchecked");
    out.interp_guard = pf.interp_guard.is_some() && !unchecked;
    out.super_unrelinked = pf.super_unrelinked.clone();
    out.indexed_metadata_pristine = pf.indexed_metadata_pristine();
    // `PERRY_FACTS_SABOTAGE=fnproto-unwritten`: claim the fact for every
    // program (the bind gap tests that write Function.prototype must fail).
    out.fnproto_bind_unwritten =
        pf.fnproto_bind_written.is_none() || pf.sabotage.as_deref() == Some("fnproto-unwritten");
    let Some(mb) = pf.bindings_for(path) else {
        return out;
    };
    if !out.interp_guard {
        for (id, params, expression) in &mb.numeric_functions {
            out.numeric_functions.insert(
                *id,
                perry_codegen::FactsNumericFunction {
                    params: params.clone(),
                    expression: perry_codegen::FactsNumericExpr::from_hir(expression)
                        .expect("validated numeric expression"),
                },
            );
        }
    }
    let stable = pf.stable_classes();
    let recv = |b: &perry_wpfacts::BindingFacts| -> Option<FactsRecv> {
        let class_ids: Vec<u32> = b
            .class_ids
            .iter()
            .copied()
            .filter(|c| stable.contains_key(c))
            .collect();
        if class_ids.is_empty() {
            return None;
        }
        let checked = !unchecked
            && (b.other
                || class_ids.len() != b.class_ids.len()
                || b.prims & !(prim::NULL | prim::UNDEF) != 0);
        Some(FactsRecv {
            class_ids,
            nullable: b.prims & (prim::NULL | prim::UNDEF) != 0,
            checked,
        })
    };
    // FVA: the bodies a binding or a field holds, when every one is a plain
    // body of this module. Interpreted code needs no guard here (decision
    // 150): a value it can hand a binding is TOP there, and then the binding
    // has no bodies. (A field is read through its proof, which declines a
    // guarded one; statics and the method tower depend on objects
    // interpreted code may write, and keep `interp`.)
    let interp = out.interp_guard;
    let bodies = |bs: &[perry_wpfacts::BodyRef]| -> Vec<FactsBody> {
        if bs.is_empty()
            || !bs.iter().all(|b| {
                b.plain && pf.module_paths.get(b.module as usize).map(String::as_str) == Some(path)
            })
        {
            return Vec::new();
        }
        bs.iter()
            .map(|b| FactsBody {
                func_id: b.func_id,
                params: b.params,
                closure: b.closure,
            })
            .collect()
    };
    for (id, b) in &mb.locals {
        if !interp {
            if let Some(o) = &b.indexed_owner {
                out.indexed_owners.insert(
                    *id,
                    perry_codegen::FactsIndexedOwner {
                        kind: o.kind,
                        min_len: o.min_len,
                        exact_len: o.exact_len,
                        allocations: o.allocations.clone(),
                    },
                );
            }
            if !b.other && b.class_ids.is_empty() && b.prims == prim::NUM {
                out.number_inputs.insert(*id);
            }
        }
        if let Some(r) = recv(b) {
            out.locals.insert(*id, r);
        }
        let fs = bodies(&b.bodies);
        if !fs.is_empty() {
            out.local_bodies.insert(*id, fs);
        }
        if let Some(cid) = b.class_object.filter(|_| !interp) {
            out.local_classes.insert(*id, cid);
        }
    }
    // FVA 3.4: every class's constant statics (a static call names its class
    // by id, which may be another module's).
    if !interp {
        for t in &pf.templates {
            if let (Some(cid), false) = (t.class_id, t.statics.is_empty()) {
                out.class_statics.insert(cid, t.statics.clone());
            }
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
            .filter(|f| (f.definite || f.declared) && f.data_field)
            .filter_map(|f| {
                Some(FactsField {
                    fns: bodies(&f.fns),
                    key: f.key.clone(),
                    slot: f.slot?,
                    readonly: f.readonly,
                    kind_mask: f.kind.mask(),
                    ta_stable: f.ta_stable,
                    ta_kind: f.ta_kind,
                    declared: f.declared,
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

/// Per-unit is the default facts mode: whole-program exact points-to does not
/// scale (fastify exceeds the work bound and does not converge unbounded);
/// owner decision 2026-10-23. `PERRY_FACTS_UNITS=0` opts into the
/// whole-program reference mode.
fn per_unit_facts(env: Option<&str>) -> bool {
    env != Some("0")
}

#[cfg(test)]
mod facts_mode_tests {
    use super::per_unit_facts;

    #[test]
    fn per_unit_is_the_default_mode() {
        // per-unit is the default: whole-program exact points-to does not
        // scale; owner 2026-10-23.
        assert!(
            per_unit_facts(None),
            "the default facts mode must be per-unit"
        );
        assert!(per_unit_facts(Some("1")));
        assert!(per_unit_facts(Some("")));
    }

    #[test]
    fn whole_program_is_an_explicit_opt_in() {
        assert!(!per_unit_facts(Some("0")));
    }
}

#[cfg(test)]
#[path = "program_facts/outlined_tests.rs"]
mod outlined_tests;

/// Borrow ordinary modules; reconstruct only outlined module entries.
fn facts_inputs(
    modules: &BTreeMap<PathBuf, perry_hir::Module>,
) -> Vec<(String, std::borrow::Cow<'_, perry_hir::Module>)> {
    modules
        .iter()
        .map(|(p, m)| {
            (
                p.display().to_string(),
                perry_codegen::codegen::entry_outline::logical_entry_module(m),
            )
        })
        .collect()
}
