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
//! - `PERRY_FACTS_UNITS=1`: the retiring per-unit solve (decision 151: one
//!   solve per module, the other modules unknown code,
//!   `perry_wpfacts::compute_units`), kept for its tests until it is
//!   deleted. The facts are solved over the whole program (decision 251:
//!   one solve over every module, where a body contributes once the program
//!   can run it, decision 225); owner decision 2026-11-03.
//!
//! Interpreted code (`new Function`, `eval`, `vm`) is no program-wide kill
//! (decision 150): the analysis models it as the ⊤ summary node, and when it
//! can run every proof is guarded by the runtime's interpreted-prototype-write
//! word (`ElisionFacts::interp_guard`).

use std::collections::{BTreeMap, BTreeSet, HashSet};
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

/// Modules loaded as independent program entries (`FactsConfig::roots`).
pub(crate) fn module_roots(ctx: &super::CompilationContext) -> Vec<String> {
    let mut roots: BTreeSet<PathBuf> = ctx
        .worker_url_entries
        .iter()
        .flat_map(|(lexical, canonical)| [lexical.clone(), canonical.clone()])
        .collect();
    for module in ctx.native_modules.values() {
        let mut worker_paths = HashSet::new();
        perry_hir::for_each_worker_new(module, &mut |expr| {
            if let perry_hir::Expr::WorkerNew { paths, .. } = expr {
                worker_paths.extend(paths.iter().cloned());
            }
        });
        for import in &module.imports {
            if !worker_paths.contains(&import.source) {
                continue;
            }
            if let Some(target) = import.resolved_path.as_ref().map(PathBuf::from) {
                if ctx.native_modules.contains_key(&target) {
                    roots.insert(target);
                }
            }
        }
    }
    roots
        .into_iter()
        .map(|path| path.display().to_string())
        .collect()
}

/// Every module's re-export sources resolved to compiled program modules.
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
    interpreted_sources: bool,
    dynamic_code_reach: &perry_hir::dynamic_code::DynamicCodeReach,
    wrapped: &std::collections::HashMap<PathBuf, super::types::CjsWrapDebugSource>,
    roots: Vec<String>,
    cache_dir: &Path,
    activation: Option<perry_wpfacts::Activation>,
) -> Option<Arc<perry_wpfacts::ProgramFacts>> {
    if !perry_wpfacts::enabled_from_env() {
        return None;
    }
    let started = std::time::Instant::now();
    let dump = std::env::var_os("PERRY_FACTS_DUMP").map(PathBuf::from);
    let relations = dump.is_some()
        && (std::env::var("PERRY_FACTS_RELATIONS").is_ok_and(|v| v == "1")
            || std::env::var("PERRY_FACTS_PROVENANCE").is_ok_and(|v| v == "1"));
    let cfg = perry_wpfacts::FactsConfig {
        verify_facts: perry_codegen::verify_facts::enabled(),
        dce_bodies: perry_codegen::verify_facts::dce_enabled(),
        roots,
        diagnostics: dump.is_some(),
        birth_census: std::env::var_os("PERRY_FACTS_BIRTH_CENSUS").is_some(),
        relations,
        no_int_ranges: matches!(
            std::env::var("PERRY_FACTS_INT_RANGES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_array_lengths: matches!(
            std::env::var("PERRY_FACTS_ARRAY_LENGTHS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_array_floor: matches!(
            std::env::var("PERRY_FACTS_ARRAY_FLOOR").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_array_reps: matches!(
            std::env::var("PERRY_FACTS_ARRAY_REPS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_const_arrays: matches!(
            std::env::var("PERRY_FACTS_CONST_ARRAYS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        // Off unless `=1` until VR-6 (DESIGN-p4l4 §7).
        value_records: std::env::var("PERRY_FACTS_VALUE_RECORDS").as_deref() == Ok("1"),
        no_record_signatures: matches!(
            std::env::var("PERRY_FACTS_RECORD_SIGNATURES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_array_fields: matches!(
            std::env::var("PERRY_FACTS_ARRAY_FIELDS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_exact_rules: matches!(
            std::env::var("PERRY_FACTS_EXACT_RULES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_zero_sign: matches!(
            std::env::var("PERRY_FACTS_ZERO_SIGN").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_leaving_writes: matches!(
            std::env::var("PERRY_FACTS_LEAVING_WRITES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_elem_undef: matches!(
            std::env::var("PERRY_FACTS_ELEM_UNDEF").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_const_numbers: matches!(
            std::env::var("PERRY_FACTS_CONST_NUMBERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_guard_ranges: matches!(
            std::env::var("PERRY_FACTS_GUARD_RANGES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_guard_presence: matches!(
            std::env::var("PERRY_FACTS_GUARD_PRESENCE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_forin_path: matches!(
            std::env::var("PERRY_FACTS_FORIN_PATH").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        optimistic_json_key: std::env::var("PERRY_FACTS_OPTIMISTIC_JSON_KEY").as_deref()
            == Ok("1"),
        json_positions: std::env::var("PERRY_FACTS_JSON_POSITIONS").as_deref() == Ok("1"),
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
        interpreted_sources,
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
        // The per-unit diagnostic records stream to a file beside the dump
        // as each unit is captured (appended to the dump below).
        diag_sidecar: dump.as_ref().map(|p| {
            let mut d = p.clone().into_os_string();
            d.push(".diag");
            PathBuf::from(d)
        }),
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
        // `PERRY_FACTS_ACTIVATION_CHECK=1`: the facts run their own pass over
        // the final HIR (the `Activation` invariant's detector: both passes'
        // profile lines must agree).
        let check = std::env::var("PERRY_FACTS_ACTIVATION_CHECK").is_ok_and(|v| v == "1");
        perry_wpfacts::compute_with(&list, &cfg, activation.filter(|_| !check))
    };
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    if perry_codegen::verify_facts::enabled() {
        perry_codegen::verify_facts::register_site_claims(verification_sites(&pf, None));
    }
    if let Some(path) = &dump {
        let synthetic = synthetic_module_dirs(cache_dir);
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
        let text = stable_synthetic_paths(text.into_bytes(), &synthetic);
        let written = std::fs::write(path, text).and_then(|()| {
            let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
            let mut w = StableDump {
                inner: std::io::BufWriter::new(&mut f),
                synthetic: &synthetic,
            };
            pf.append_streamed(&mut w)?;
            std::io::Write::flush(&mut w)
        });
        if let Err(e) = written {
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
        eprintln!(
            "perry: wpfacts: array @@iterator {}; array iterator next/return {}",
            pf.array_iteration_written
                .as_deref()
                .map_or("unwritten".to_string(), |r| format!("may be written: {r}")),
            pf.array_iterator_close_written
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
            if s.widened_stop {
                ", widened past the work bound, then OVER 1000 times it: no facts"
            } else if s.aborted {
                ", OVER the work/time bound: no facts"
            } else if s.widened {
                ", past the work bound: widened"
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
    // The bodies this module's code is removed for: its object depends on
    // them, so they join the digest (only with removal on and some inactive).
    let inactive: Vec<(u32, &str)> = if perry_codegen::verify_facts::dce_enabled() {
        pf.verification_bodies
            .iter()
            .flatten()
            .filter(|((path, _, _), active)| path == &key && !**active)
            .map(|((_, id, name), _)| (*id, name.as_str()))
            .collect()
    } else {
        Vec::new()
    };
    let digest = if elision.is_empty()
        && !elision.fnproto_bind_unwritten
        && elision.super_unrelinked.is_empty()
        && !elision.fnproto_borrow_unwritten
        && inactive.is_empty()
    {
        pf.digest_for(&key)
    } else {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pf.digest_for(&key).hash(&mut h);
        elision.hash(&mut h);
        inactive.hash(&mut h);
        h.finish()
    };
    perry_codegen::ProgramFactsView {
        fallback: super::fallback_diag::facts_for(pf, &key),
        verification_bodies: if perry_codegen::verify_facts::enabled()
            || perry_codegen::verify_facts::dce_enabled()
        {
            pf.verification_bodies.as_ref().map(|b| {
                Arc::new(
                    b.iter()
                        .filter(|((path, _, _), _)| path == &key)
                        .map(|((_, id, name), active)| ((*id, name.clone()), *active))
                        .collect(),
                )
            })
        } else {
            None
        },
        verification_module: (perry_codegen::verify_facts::enabled()
            || perry_codegen::verify_facts::dce_enabled())
        .then(|| key.clone()),
        verification_sites: perry_codegen::verify_facts::enabled()
            .then(|| Arc::new(verification_sites(pf, Some(&key)))),
        digest,
        dynamic_code_reach: pf.dynamic_code_reach.clone(),
        // Set on the entry module by the driver, once R is closed.
        realm_reach: None,
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
    // DESIGN-p4l4 VR-2: the admitted value records' classes, for check mode
    // only (`expr/record_witness.rs`); empty with the switch off.
    out.record_classes = pf
        .templates
        .iter()
        .filter(|t| t.record.is_ok())
        .filter_map(|t| t.class_id)
        .collect();
    out.super_unrelinked = pf.super_unrelinked.clone();
    out.zero_sign_tolerated = pf.zero_sign_tolerated > 0;
    out.indexed_metadata_pristine = pf.indexed_metadata_pristine();
    // `PERRY_FACTS_SABOTAGE=fnproto-unwritten`: claim the fact for every
    // program (the bind gap tests that write Function.prototype must fail).
    out.fnproto_bind_unwritten =
        pf.fnproto_bind_written.is_none() || pf.sabotage.as_deref() == Some("fnproto-unwritten");
    // `PERRY_FACTS_SABOTAGE=array-iteration-unwritten`: claim both array
    // iteration facts for every program (the for-of gap tests that replace
    // an iterator, `next` or `return` must fail).
    let iteration_sabotage = pf.sabotage.as_deref() == Some("array-iteration-unwritten");
    out.array_iteration_unwritten =
        iteration_sabotage || (pf.interp_guard.is_none() && pf.array_iteration_written.is_none());
    out.array_iterator_close_unwritten = iteration_sabotage
        || (pf.interp_guard.is_none() && pf.array_iterator_close_written.is_none());
    out.fnproto_borrow_unwritten = pf.fnproto_borrow_written.is_none() || unchecked;
    // The method calls proven to read their intrinsic member. Interpreted
    // code may store into any member it can name: then no site is proven and
    // each reads its property (a guarded namespace call has tested the
    // namespace shadow bit instead).
    if !out.interp_guard {
        if let Some(module) = pf.module_paths.iter().position(|p| p == path) {
            out.native_member_calls = pf
                .sites
                .iter()
                .filter(|s| {
                    s.module as usize == module
                        && s.is_call
                        && matches!(s.verdict, perry_wpfacts::Verdict::NativeMember)
                })
                .map(|s| s.byte_offset)
                .collect();
        }
    }
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
    if !interp {
        for (offset, iv) in &mb.call_arg_ranges {
            out.call_arg_ranges.insert(*offset, iv.clone());
        }
        out.number_calls.extend(mb.number_calls.iter().copied());
        out.call_kinds
            .extend(mb.call_kinds.iter().map(|&(o, e, k)| ((o, e), k)));
        out.number_element_reads
            .extend(mb.number_element_reads.iter().copied());
        out.int32_element_reads
            .extend(mb.int32_element_reads.iter().copied());
        out.element_floors.extend(mb.element_floors.iter().copied());
        out.const_numbers.extend(mb.const_numbers.iter().copied());
        out.element_keys.extend(mb.element_keys.iter().copied());
        out.key_signs.extend(mb.key_signs.iter().copied());
        out.wide_keys.extend(mb.wide_keys.iter().copied());
        out.inbounds_stores
            .extend(mb.inbounds_stores.iter().copied());
        out.rest_reads.extend(mb.rest_reads.iter().copied());
        out.rest_writes.extend(mb.rest_writes.iter().copied());
        out.rest_births.extend(mb.rest_births.iter().copied());
        out.rest_reserves.extend(mb.rest_reserves.iter().copied());
        out.list_capacities
            .extend(mb.list_capacities.iter().copied());
        out.rest_pushes.extend(mb.rest_pushes.iter().copied());
        out.fill_push_recvs.extend(mb.fill_push_recvs.iter().copied());
        out.fill_births.extend(mb.fill_births.iter().copied());
        out.rest_lane_reads.extend(mb.rest_lane_reads.iter().copied());
        // DESIGN-p4l4 VR-4b: the record arrays. Their record's layout is
        // this module's (wpfacts rejects a record array held in another
        // module); without it codegen refuses the module.
        {
            let mut layout = |r: u32| {
                if let Some(l) = record_layout(pf, path, r) {
                    out.record_layouts.insert(r, l);
                }
                r
            };
            for (id, r) in &mb.record_array_locals {
                out.record_array_locals.insert(*id, layout(*r));
            }
            for (at, k, r) in &mb.record_array_reads {
                out.record_array_reads.insert((*at, k.clone()), layout(*r));
            }
            for (id, r) in &mb.record_array_birth_locals {
                out.record_array_birth_locals.insert(*id, layout(*r));
            }
            for (at, r) in &mb.record_array_birth_stores {
                out.record_array_birth_stores.insert(*at, layout(*r));
            }
            for (c, k, r) in &mb.record_array_birth_fields {
                out.record_array_birth_fields
                    .insert((*c, k.clone()), layout(*r));
            }
        }
        let rep = |r: &perry_wpfacts::Rep| facts_rep(*r);
        for (id, sig) in &mb.signatures {
            // DESIGN-p4l4 VR-3b: a Record slot whose layout this module
            // lowers (`record_layout`: the literal is born here).
            let mut record = |r: &perry_wpfacts::Rep| match r {
                perry_wpfacts::Rep::Record(r) => {
                    let layout = record_layout(pf, path, r.tmpl)?;
                    out.record_layouts.insert(r.tmpl, layout);
                    Some(perry_codegen::FactsRecRep {
                        tmpl: r.tmpl,
                        opt: r.opt,
                    })
                }
                _ => None,
            };
            let mut record_params: Vec<_> = sig.params.iter().map(&mut record).collect();
            if record_params.iter().all(Option::is_none) {
                record_params.clear();
            }
            let record_ret = record(&sig.ret);
            let sig = perry_codegen::FactsSignature {
                params: sig.params.iter().map(rep).collect(),
                ranges: sig.ranges.clone(),
                ret: rep(&sig.ret),
                this_class: sig.this_class,
                zero_ok: pf.zero_sign_tolerated > 0,
                record_params,
                record_ret,
            };
            // A signature whose every slot is Boxed here (a record this
            // module does not lower) is none.
            if sig.record_ret.is_none()
                && sig.record_params.is_empty()
                && sig.this_class.is_none()
                && sig.ret.encoding() == perry_codegen::FactsEnc::Boxed
                && sig
                    .params
                    .iter()
                    .all(|r| r.encoding() == perry_codegen::FactsEnc::Boxed)
            {
                continue;
            }
            out.signatures.insert(*id, sig);
        }
        // Same-module typed calls only (DESIGN-p2 rev 2.2 §2).
        let module_index = pf.module_paths.iter().position(|p| p == path);
        for (offset, (callee_module, func_id)) in &mb.typed_calls {
            if Some(*callee_module as usize) == module_index {
                out.typed_calls.insert(*offset, *func_id);
            }
        }
        for (offset, c) in &mb.field_read_calls {
            let default = match &c.default {
                None => None,
                Some(perry_wpfacts::FieldReadDefault::Number(bits)) => Some(*bits),
                // Codegen applies a Number default only.
                Some(_) => continue,
            };
            out.field_read_calls.insert(
                *offset,
                perry_codegen::FactsFieldReadCall {
                    field: c.field.clone(),
                    index_arg: c.index_arg,
                    default,
                    element_offset: c.element_offset,
                    field_offset: c.field_offset,
                    in_floor: c.array.filter(|_| c.in_floor).map(|r| r.floor),
                    rest: c.rest,
                },
            );
        }
        if element_reads_enabled() {
            for (offset, r) in &mb.element_reads {
                out.element_reads.insert(
                    *offset,
                    perry_codegen::FactsElementRead {
                        len: r.len,
                        key_in_bounds: r.key_in_bounds,
                    },
                );
            }
        }
    }
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
            // A Number or `undefined` and nothing else (the negative control
            // `undefnum` claims it for anything that may be a Number).
            if b.prims & prim::NUM != 0
                && (!b.other && b.class_ids.is_empty() && b.prims & !(prim::NUM | prim::UNDEF) == 0
                    || std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok("undefnum"))
            {
                out.undef_number_inputs.insert(*id);
            }
            if !b.other && b.class_ids.is_empty() && b.prims == prim::NUM {
                out.number_inputs.insert(*id);
                // A Number binding whose interval proof lies in the int32
                // range: every value it holds is an int32 (never -0).
                if b.int_range
                    .is_some_and(|(lo, hi)| lo >= i64::from(i32::MIN) && hi <= i64::from(i32::MAX))
                {
                    out.int32_inputs.insert(*id);
                    if let Some(range) = b.int_range {
                        out.int32_ranges.insert(*id, range);
                    }
                    if b.zero_tol {
                        out.zero_tolerant_locals.insert(*id);
                    }
                    if b.int_range.is_some_and(|(lo, _)| lo >= 0) {
                        out.nonneg_int32_inputs.insert(*id);
                    }
                }
            }
            out.local_reps.insert(*id, facts_rep(b.rep));
            // DESIGN-p4l4 VR-3a: a binding of one admitted value record is
            // its fields by value (`perry_codegen::value_records`). The
            // template's literal must be born in this module (its class is
            // this module's, or it stays an `Expr::Object`).
            if let perry_wpfacts::Rep::Record(r) = b.rep {
                if let Some(layout) = record_layout(pf, path, r.tmpl) {
                    out.record_layouts.insert(r.tmpl, layout);
                    out.record_locals.insert(
                        *id,
                        perry_codegen::FactsRecRep {
                            tmpl: r.tmpl,
                            opt: r.opt,
                        },
                    );
                }
            }
            if b.length_number {
                out.length_number_inputs.insert(*id);
            }
            if let Some(n) = b.const_length {
                out.const_length_inputs.insert(*id, n);
            }
            if b.immutable_array {
                out.immutable_arrays.insert(*id);
            }
            if b.literal_strings {
                out.literal_string_locals.insert(*id);
            }
            if b.stable_length {
                out.stable_length_locals.insert(*id, b.stable_front);
            }
            if let Some(c) = &b.const_elems {
                out.const_arrays.insert(*id, c.clone());
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
    // Holder facts: the prefix-stable classes a definite data field holds
    // (a reserved key may still be absent: no field-class fact),
    // when it holds nothing else but null/undefined (no ⊤, function,
    // builtin, other primitive, literal or class without stable facts).
    // The field kind is the join over every unit's stores, so per unit it
    // is never stronger than whole. Interpreted code can store anything.
    let holder_classes = |f: &perry_wpfacts::FieldFacts| -> Option<(Vec<u32>, bool)> {
        let k = &f.kind;
        if interp
            || f.declared
            || f.reserved
            || !f.definite
            || !f.data_field
            || k.top
            || k.funcs != 0
            || k.builtin_objs != 0
            || k.objs.is_empty()
            || k.prims & !(prim::NULL | prim::UNDEF) != 0
        {
            return None;
        }
        let ids = pf.classes_of(k)?;
        if ids.is_empty() || !ids.iter().all(|c| stable.contains_key(c)) {
            return None;
        }
        Some((ids, k.prims != 0))
    };
    // Step 5b: a typed body's entry check proves a class set for a
    // parameter no binding fact covers, so every prefix-stable class's
    // layout facts are visible, not only those of classes some binding holds.
    for (&cid, &t) in &stable {
        let fields = t
            .fields
            .iter()
            // A private field's row is not codegen's: codegen finds a
            // private field by its completed shape (#11791), and its view
            // looks a public key up by spelling (`ElisionFacts::field`).
            .filter(|f| (f.definite || f.declared || f.reserved) && f.data_field && !f.private)
            .filter_map(|f| {
                let (classes, nullable) = holder_classes(f).unwrap_or_default();
                if f.zero_tol {
                    out.zero_tolerant_fields.insert((cid, f.key.clone()));
                }
                if f.rest {
                    out.rest_keys.insert(f.key.clone());
                }
                // Interpreted code can store any string.
                if f.literal_strings && !interp {
                    out.literal_string_fields.insert((cid, f.key.clone()));
                }
                Some(FactsField {
                    classes,
                    nullable,
                    fns: bodies(&f.fns),
                    key: f.key.clone(),
                    slot: f.slot?,
                    readonly: f.readonly,
                    kind_mask: f.kind.mask(),
                    ta_stable: f.ta_stable,
                    ta_kind: f.ta_kind,
                    declared: f.declared,
                    reserved: f.reserved,
                    rep: facts_rep(f.rep),
                    rest: f.rest,
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

/// The facts are solved over the whole program (decision 251, owner
/// 2026-11-03): one solve over every module, where a body contributes once
/// the program can run it (decision 225). `PERRY_FACTS_UNITS=1` selects the
/// retiring per-unit solve, kept for its tests until it is deleted; no other
/// value selects anything.
fn per_unit_facts(env: Option<&str>) -> bool {
    env == Some("1")
}

/// The directories compiler-generated modules (the CommonJS support module,
/// script strings, native ESM namespaces) are written to: `<cache dir>/
/// synthetic-modules`, as given and canonical (the modules are keyed by their
/// canonical path).
fn synthetic_module_dirs(cache_dir: &Path) -> Vec<Vec<u8>> {
    let dir = cache_dir.join("synthetic-modules");
    let mut dirs = Vec::new();
    if let Ok(canonical) = dir.canonicalize() {
        dirs.push(canonical);
    }
    dirs.push(dir);
    dirs.dedup();
    dirs.iter()
        .map(|d| d.to_string_lossy().into_owned().into_bytes())
        .collect()
}

/// A synthetic module's name is its file under the cache dir, so the same
/// program dumped from two cache dirs would differ in every line naming one.
/// The dump names it relative to the cache dir instead (the file name is a
/// hash of the entry, stable across cache dirs).
fn stable_synthetic_paths(mut bytes: Vec<u8>, synthetic: &[Vec<u8>]) -> Vec<u8> {
    const STABLE: &[u8] = b"<cache-dir>/synthetic-modules";
    for dir in synthetic {
        if dir.is_empty() {
            continue;
        }
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i..].starts_with(dir) {
                out.extend_from_slice(STABLE);
                i += dir.len();
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        bytes = out;
    }
    bytes
}

/// The streamed per-unit diagnostics go through the same rewrite.
struct StableDump<'a, W: std::io::Write> {
    inner: W,
    synthetic: &'a [Vec<u8>],
}

impl<W: std::io::Write> std::io::Write for StableDump<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write_all(buf)?;
        Ok(buf.len())
    }
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.inner
            .write_all(&stable_synthetic_paths(buf.to_vec(), self.synthetic))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod stable_dump_tests {
    use super::*;

    #[test]
    fn the_cache_dir_is_not_part_of_a_synthetic_module_name() {
        let a = synthetic_module_dirs(Path::new("/tmp/c-a"));
        let b = synthetic_module_dirs(Path::new("/var/x/c-b"));
        let line =
            |d: &str| format!("M 3 {d}/synthetic-modules/perry-cjs-00ff.mjs\nF /src/app.ts\n");
        let na = stable_synthetic_paths(line("/tmp/c-a").into_bytes(), &a);
        let nb = stable_synthetic_paths(line("/var/x/c-b").into_bytes(), &b);
        assert_eq!(na, nb);
        assert!(String::from_utf8(na)
            .unwrap()
            .contains("<cache-dir>/synthetic-modules/perry-cjs-00ff.mjs"));
    }
}

#[cfg(test)]
mod facts_mode_tests {
    use super::per_unit_facts;

    #[test]
    fn whole_program_is_the_default_mode() {
        // decision 251: the whole solve is the one codegen reads.
        assert!(
            !per_unit_facts(None),
            "the default facts mode must be whole-program"
        );
        assert!(!per_unit_facts(Some("0")));
        assert!(!per_unit_facts(Some("")));
    }

    #[test]
    fn per_unit_is_an_explicit_opt_in() {
        assert!(per_unit_facts(Some("1")));
    }
}

#[cfg(test)]
#[path = "program_facts/outlined_tests.rs"]
mod outlined_tests;

#[cfg(test)]
#[path = "program_facts/literal_chunks_tests.rs"]
mod literal_chunks_tests;

/// Borrow ordinary modules; reconstruct only outlined module entries.
/// Dead-code elimination (`dynamic_reach`): the dynamic `import()` /
/// `require(expr)` specifiers a body that can run holds, by the whole
/// program's activation pass over `modules`, as (module path, specifier).
/// `None` when facts are off or past their bounds (every site may run).
/// The compile's one activation pass (DECISIONS 225, `perry_wpfacts::
/// Activation`): dynamic reach reads its live `import()` sites, the facts
/// take it over (`run`). `None` with the facts off.
pub(crate) fn activation(
    modules: &BTreeMap<PathBuf, perry_hir::Module>,
    export_sources: perry_wpfacts::ExportSources,
    dynamic_code: bool,
    interpreted_sources: bool,
    dynamic_code_reach: &perry_hir::dynamic_code::DynamicCodeReach,
    roots: Vec<String>,
) -> Option<perry_wpfacts::Activation> {
    if !perry_wpfacts::enabled_from_env() {
        return None;
    }
    let cfg = perry_wpfacts::FactsConfig {
        roots,
        // The facts' negative controls act on this pass (it is theirs).
        no_int_ranges: matches!(
            std::env::var("PERRY_FACTS_INT_RANGES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_array_lengths: matches!(
            std::env::var("PERRY_FACTS_ARRAY_LENGTHS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_array_floor: matches!(
            std::env::var("PERRY_FACTS_ARRAY_FLOOR").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_array_reps: matches!(
            std::env::var("PERRY_FACTS_ARRAY_REPS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_const_arrays: matches!(
            std::env::var("PERRY_FACTS_CONST_ARRAYS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        // Off unless `=1` until VR-6 (DESIGN-p4l4 §7).
        value_records: std::env::var("PERRY_FACTS_VALUE_RECORDS").as_deref() == Ok("1"),
        no_record_signatures: matches!(
            std::env::var("PERRY_FACTS_RECORD_SIGNATURES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_array_fields: matches!(
            std::env::var("PERRY_FACTS_ARRAY_FIELDS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_exact_rules: matches!(
            std::env::var("PERRY_FACTS_EXACT_RULES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_zero_sign: matches!(
            std::env::var("PERRY_FACTS_ZERO_SIGN").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_leaving_writes: matches!(
            std::env::var("PERRY_FACTS_LEAVING_WRITES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_elem_undef: matches!(
            std::env::var("PERRY_FACTS_ELEM_UNDEF").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_const_numbers: matches!(
            std::env::var("PERRY_FACTS_CONST_NUMBERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_guard_ranges: matches!(
            std::env::var("PERRY_FACTS_GUARD_RANGES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_guard_presence: matches!(
            std::env::var("PERRY_FACTS_GUARD_PRESENCE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        no_forin_path: matches!(
            std::env::var("PERRY_FACTS_FORIN_PATH").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        optimistic_json_key: std::env::var("PERRY_FACTS_OPTIMISTIC_JSON_KEY").as_deref()
            == Ok("1"),
        json_positions: std::env::var("PERRY_FACTS_JSON_POSITIONS").as_deref() == Ok("1"),
        sabotage: std::env::var("PERRY_FACTS_SABOTAGE")
            .ok()
            .filter(|s| !s.is_empty()),
        profile: std::env::var("PERRY_FACTS_PROFILE").is_ok_and(|v| v == "1"),
        budget_secs: std::env::var("PERRY_FACTS_BUDGET_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|&s| s != 0),
        // The activation pass alone, over the whole program: a larger bound
        // than the per-unit facts' (the Supabase CLI needs ~20x the default).
        max_work: Some(
            std::env::var("PERRY_DYNAMIC_REACH_MAX_WORK")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_MAX_WORK * 100),
        ),
        // Its points-to bound too: with every re-export source walked, the
        // Supabase CLI converges at ~790M entries.
        max_pts: Some(
            std::env::var("PERRY_FACTS_MAX_PTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1_000_000_000),
        ),
        export_sources,
        dynamic_code_reach: dynamic_code_reach.clone(),
        interpreted_sources,
        dynamic_code: dynamic_code.then(|| {
            "E14 the program can run interpreted code (deferred new Function/eval or node:vm)"
                .into()
        }),
        unit: false,
        jobs: perry_codegen::workers::unit_workers(),
        ..Default::default()
    };
    let inputs = facts_inputs(modules);
    let list: Vec<(String, &perry_hir::Module)> = inputs
        .iter()
        .map(|(p, m)| (p.clone(), m.as_ref()))
        .collect();
    Some(perry_wpfacts::activation(&list, &cfg))
}

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

#[cfg(test)]
#[path = "program_facts/worker_roots_tests.rs"]
mod worker_roots_tests;

#[cfg(test)]
#[path = "program_facts/native_addon_tests.rs"]
mod native_addon_tests;

#[cfg(test)]
#[path = "program_facts/element_tests.rs"]
mod element_tests;

fn verification_sites(
    pf: &perry_wpfacts::ProgramFacts,
    path: Option<&str>,
) -> Vec<perry_codegen::verify_facts::SiteClaim> {
    use perry_codegen::verify_facts::{SiteClaim, SiteVerdict};
    pf.sites
        .iter()
        .enumerate()
        .filter_map(|(ordinal, site)| {
            let module = pf.module_paths.get(site.module as usize)?;
            if path.is_some_and(|path| path != module) {
                return None;
            }
            let verdict = match site.verdict {
                perry_wpfacts::Verdict::OwnLength => SiteVerdict::OwnLength,
                perry_wpfacts::Verdict::NativeMember => SiteVerdict::NativeMember,
                perry_wpfacts::Verdict::T0Mono => SiteVerdict::T0Mono,
                perry_wpfacts::Verdict::T0Set => SiteVerdict::T0Set,
                perry_wpfacts::Verdict::E0 => SiteVerdict::E0,
                perry_wpfacts::Verdict::Dead => SiteVerdict::Dead,
                perry_wpfacts::Verdict::OwnDataOrAbsent => SiteVerdict::OwnDataOrAbsent,
                perry_wpfacts::Verdict::NsExport => SiteVerdict::NsExport,
                perry_wpfacts::Verdict::IntrinsicCall => SiteVerdict::IntrinsicCall,
                perry_wpfacts::Verdict::No(_) => return None,
            };
            Some(SiteClaim {
                ordinal,
                module: module.clone(),
                byte_offset: site.byte_offset.checked_sub(1),
                key: site.key.clone(),
                is_call: site.is_call,
                bodies: site.bodies.clone(),
                verdict,
            })
        })
        .collect()
}

/// `PERRY_WP_ELEMENT_READS=0` withholds wpfacts' proven element reads
/// (`ModuleBindings::element_reads`): every element read keeps its guarded
/// tier (representation RFC §10.7, P4).
fn element_reads_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_WP_ELEMENT_READS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The layout codegen lowers an admitted value record of template `tmpl`
/// with, when its literal is born in module `path` (DESIGN-p4l4 VR-3a).
fn record_layout(
    pf: &perry_wpfacts::ProgramFacts,
    path: &str,
    tmpl: u32,
) -> Option<perry_codegen::FactsRecordLayout> {
    let t = match pf.templates.get(tmpl as usize) {
        Some(t) if t.id == tmpl => t,
        _ => pf.templates.iter().find(|t| t.id == tmpl)?,
    };
    let layout = t.record.as_ref().ok()?;
    let module = pf.module_paths.iter().position(|p| p == path)?;
    if t.module != Some(module as u32) {
        return None;
    }
    Some(perry_codegen::FactsRecordLayout {
        class_id: t.class_id,
        keys: layout.fields.iter().map(|(k, _)| k.clone()).collect(),
        numeric: layout
            .fields
            .iter()
            .map(|(_, r)| {
                matches!(
                    r,
                    perry_wpfacts::records::FieldRep::F64 | perry_wpfacts::records::FieldRep::I32
                )
            })
            .collect(),
        int32: layout
            .fields
            .iter()
            .map(|(_, r)| matches!(r, perry_wpfacts::records::FieldRep::I32))
            .collect(),
    })
}

/// wpfacts `Rep` as codegen's `FactsRep`, one to one.
fn facts_rep(r: perry_wpfacts::Rep) -> perry_codegen::FactsRep {
    use perry_codegen::{FactsArrRep, FactsElemRep, FactsRep};
    use perry_wpfacts::{ElemRep, Rep};
    match r {
        Rep::Boxed => FactsRep::Boxed,
        Rep::F64 => FactsRep::F64,
        Rep::I32 => FactsRep::I32,
        Rep::Arr(a) => FactsRep::Arr(FactsArrRep {
            elem: match a.elem {
                ElemRep::Boxed => FactsElemRep::Boxed,
                ElemRep::F64 => FactsElemRep::F64,
                ElemRep::I32 => FactsElemRep::I32,
            },
            floor: a.floor,
            dense: a.dense,
            fixed: a.fixed,
            im: a.im,
            // Negative control `restrep` (I32 at rest B2): every int Array
            // is claimed at rest, so a rep-keyed load decodes a double word.
            at_rest: a.at_rest
                || (a.elem == ElemRep::I32
                    && std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok("restrep")),
        }),
        // DESIGN-p4l4 VR-2: codegen lowers records from VR-3; until then a
        // record is the Boxed word it is today.
        Rep::Record(_) => FactsRep::Boxed,
    }
}
