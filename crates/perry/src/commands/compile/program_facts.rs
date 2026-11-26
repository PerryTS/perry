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
//! - The facts are solved over the whole program (decision 251: one solve
//!   over every module, where a body contributes once the program can run
//!   it, decision 225); owner decision 2026-11-03. The per-unit mode
//!   (`PERRY_FACTS_UNITS`) is retired; a set variable is ignored.
//!
//! Interpreted code (`new Function`, `eval`, `vm`) is no program-wide kill
//! (decision 150): the analysis models it as the ⊤ summary node, and when it
//! can run every proof is guarded by the runtime's interpreted-prototype-write
//! word (`ElisionFacts::interp_guard`).

use perry_wpfacts::elide_enabled;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Solve-work bound (see `perry_wpfacts::FactsConfig::max_work`):
/// 1,000 million (owner 2026-11-15; was 200 million). With `this` bound per
/// (receiver, key), shared accessor walks and groups, tsc needed 45M, fastify 59M, Effect 27M,
/// zod 3.9M, commander 1.8M; the Next.js server (~3G) still stops at the
/// bound and gets no facts.
const DEFAULT_MAX_WORK: u64 = 1_000_000_000;

/// ASSUMED (owner 2026-11-16, "Assume it for now", interim until an exact
/// link-time check of each library's imports): the native-library externs
/// whose manifest parameters and result are all primitives, with the JS kind
/// of the result, keyed as the codegen's `ffi_signatures` (a later manifest
/// entry of a name replaces an earlier one). Perry lowers such a parameter by
/// a check (`js_native_abi_check_*`: a TypeError, never ToNumber / ToString)
/// or by ToBoolean (`bool`), so no program code runs at the call
/// (perry-codegen `lower_call/extern_func.rs`, `lower_manifest_param`).
/// `PERRY_FACTS_NATIVE_EXTERN=0`: none (every extern call is unknown code).
pub(crate) fn native_externs<'a>(
    functions: impl Iterator<Item = &'a super::types::NativeFunctionDecl>,
) -> BTreeMap<String, perry_wpfacts::NativeResult> {
    use perry_api_manifest::NativeAbiType as T;
    use perry_wpfacts::NativeResult as K;
    let mut out = BTreeMap::new();
    if std::env::var("PERRY_FACTS_NATIVE_EXTERN").is_ok_and(|v| v == "0") {
        return out;
    }
    let numeric = |t: &T| {
        matches!(
            t,
            T::F64
                | T::F32
                | T::I8
                | T::I16
                | T::I32
                | T::I64
                | T::U8
                | T::U16
                | T::U32
                | T::U64
                | T::USize
                | T::ISize
        )
    };
    for f in functions {
        let result = match &f.returns {
            T::Void => Some(K::Undefined),
            T::String | T::I64String => Some(K::String),
            T::Bool => Some(K::Boolean),
            t if numeric(t) => Some(K::Number),
            _ => None,
        };
        let params = f
            .params
            .iter()
            .all(|t| numeric(t) || matches!(t, T::String | T::I64String | T::Bool));
        match result {
            Some(r) if params => {
                out.insert(f.name.clone(), r);
            }
            _ => {
                out.remove(&f.name);
            }
        }
    }
    out
}

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

/// The count a bound variable sets (`PERRY_FACTS_MAX_WORK` ...): `None`
/// when unset or empty. A value that is no count stops the compile with an
/// error: a bound that silently falls back to its default measures
/// something else than was asked for.
fn env_count(name: &str) -> Option<u64> {
    let v = std::env::var(name).ok().filter(|v| !v.trim().is_empty())?;
    match parse_count(&v) {
        Some(n) => Some(n),
        None => {
            eprintln!(
                "error: {name}={v} is not a count (a non-negative integer: 1000000000, 1_000_000_000 or 1e9)"
            );
            std::process::exit(2);
        }
    }
}

/// A non-negative integer written as digits (`_` separators allowed) or in
/// scientific notation (`1e10`, `2.5E9`) whose value is a whole number that
/// fits in a `u64`.
fn parse_count(v: &str) -> Option<u64> {
    let v = v.trim().replace('_', "");
    if let Ok(n) = v.parse::<u64>() {
        return Some(n);
    }
    let (m, e) = v.split_once(['e', 'E'])?;
    let e: u32 = e.strip_prefix('+').unwrap_or(e).parse().ok()?;
    let (int, frac) = m.split_once('.').unwrap_or((m, ""));
    if int.is_empty() && frac.is_empty()
        || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit())
    {
        return None;
    }
    // mantissa digits x 10^(e - fraction digits), exactly.
    let digits = format!("{int}{frac}");
    let shift = e.checked_sub(frac.len() as u32);
    let digits = match shift {
        Some(k) => format!("{digits}{}", "0".repeat(k as usize)),
        None => {
            let cut = (frac.len() as u32 - e) as usize;
            let (keep, rest) = digits.split_at(digits.len() - cut);
            if rest.bytes().any(|b| b != b'0') {
                return None;
            }
            keep.to_string()
        }
    };
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some(0);
    }
    digits.parse::<u64>().ok()
}

pub(crate) fn run(
    modules: &BTreeMap<PathBuf, perry_hir::Module>,
    export_sources: perry_wpfacts::ExportSources,
    dynamic_code: bool,
    data_url_import: bool,
    dynamic_code_reach: &perry_hir::dynamic_code::DynamicCodeReach,
    wrapped: &std::collections::HashMap<PathBuf, super::types::CjsWrapDebugSource>,
    roots: Vec<String>,
    cache_dir: &Path,
    activation: Option<perry_wpfacts::Activation>,
    native_externs: BTreeMap<String, perry_wpfacts::NativeResult>,
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
        namespace_pairs: namespace_pairs_on(),
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
        // Off unless `=1` until VR-6.
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
        optimistic_json_key: std::env::var("PERRY_FACTS_OPTIMISTIC_JSON_KEY").as_deref() == Ok("1"),
        json_positions: std::env::var("PERRY_FACTS_JSON_POSITIONS").as_deref() == Ok("1"),
        sabotage: std::env::var("PERRY_FACTS_SABOTAGE")
            .ok()
            .filter(|s| !s.is_empty()),
        // Past either bound the program simply gets no facts (sound).
        budget_secs: env_count("PERRY_FACTS_BUDGET_SECS").filter(|&s| s != 0),
        max_work: Some(env_count("PERRY_FACTS_MAX_WORK").unwrap_or(DEFAULT_MAX_WORK)),
        profile: std::env::var("PERRY_FACTS_PROFILE").is_ok_and(|v| v == "1"),
        // Points-to entries bound (memory); past it the program gets no facts.
        max_pts: Some(env_count("PERRY_FACTS_MAX_PTS").unwrap_or(2_000_000_000)),
        export_sources,
        explain: std::env::var("PERRY_FACTS_EXPLAIN").is_ok_and(|v| v == "1")
            || perry_codegen::fallback_diag::enabled(),
        dynamic_code_reach: dynamic_code_reach.clone(),
        data_url_import,
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
        native_externs,
    };
    // Entry outlining moves module bindings into compiler-owned functions.
    // Facts need the same logical module scope that codegen's globals and
    // export getters use; retain the emitted HIR unchanged for codegen.
    let inputs = facts_inputs(modules);
    let list: Vec<(String, &perry_hir::Module)> = inputs
        .iter()
        .map(|(p, m)| (p.clone(), m.as_ref()))
        .collect();
    // `PERRY_FACTS_ACTIVATION_CHECK=1`: the facts run their own pass over
    // the final HIR (the `Activation` invariant's detector: both passes'
    // profile lines must agree).
    let check = std::env::var("PERRY_FACTS_ACTIVATION_CHECK").is_ok_and(|v| v == "1");
    let pf = perry_wpfacts::compute_with(&list, &cfg, activation.filter(|_| !check));
    // The blocker ledger (`PERRY_FACTS_LEDGER`) solved with the realm never
    // held: its facts never reach codegen; the compile stops here.
    if std::env::var_os("PERRY_FACTS_LEDGER").is_some_and(|v| !v.is_empty()) {
        eprintln!("perry: wpfacts: ledger written; compile stopped (PERRY_FACTS_LEDGER)");
        std::process::exit(0);
    }
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
            "perry: wpfacts: typed-array metadata {}; Array/Object.prototype {}",
            pf.typed_metadata_written
                .as_deref()
                .map_or("unwritten".to_string(), |r| format!("may be written: {r}")),
            pf.array_methods_written
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
            .filter(|s| !s.access.is_store() && !matches!(s.verdict, perry_wpfacts::Verdict::No(_)))
            .count();
        let reads = pf.sites.iter().filter(|s| !s.access.is_store()).count();
        // ACCSITE1 (owner 11-15): stores in their own column; AddsKey is
        // unproven in round one (D1).
        let st_all = pf.sites.iter().filter(|s| s.access.is_store()).count();
        let st_ok = pf
            .sites
            .iter()
            .filter(|s| {
                s.access.is_store()
                    && !matches!(
                        s.verdict,
                        perry_wpfacts::Verdict::No(_) | perry_wpfacts::Verdict::AddsKey
                    )
            })
            .count();
        // R9BIRTH4 (owner 11-15): the known-object stores apart.
        let st_known = pf
            .sites
            .iter()
            .filter(|s| matches!(s.verdict, perry_wpfacts::Verdict::KnownObjectStore))
            .count();
        eprintln!("perry: wpfacts: {st_known} known-object stores");
        eprintln!(
            "perry: wpfacts: {} sites rest on the native-extern assumption",
            pf.native_extern_sites.len()
        );
        eprintln!(
            "perry: wpfacts: {} modules, {} templates ({} fact units), {} nodes, {} constraints; \
             {} of {} read/call sites proven, {} of {} stores proven; gen {:.0} ms, solve {:.0} ms ({} pops, work {}{}), derive {:.0} ms, wall {:.0} ms",
            s.modules,
            s.templates,
            s.fact_units,
            s.nodes,
            s.constraints,
            t0,
            reads,
            st_ok,
            st_all,
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
    }
    if std::env::var("PERRY_FACTS_ONLY").is_ok_and(|v| v == "1") {
        // Analysis-only run (precision reports, checkers): stop after the
        // dump, before codegen.
        std::process::exit(0);
    }
    Some(Arc::new(pf))
}

/// ACCBUILD3: the symbol of the accessor function `func` (a getter when
/// `!set`) for `key` of class `class_id` defined in `module`, exactly as that
/// module's codegen names it (`method_registry.rs`: `scoped_method_name(
/// module prefix, class name, "__set_" + setter fn name)`). `None` unless the
/// module holds one class with that id and name whose own `key` accessor is
/// the function `func` and the class has no evaluated or static accessor of
/// that name.
pub(crate) fn accessor_symbol_in(
    hir: &perry_hir::Module,
    prefix: &str,
    class_id: u32,
    func: u32,
    key: &str,
    set: bool,
) -> Option<String> {
    let mut classes = hir.classes.iter().filter(|c| c.id == class_id);
    let c = classes.next()?;
    if classes.next().is_some()
        || hir.classes.iter().filter(|o| o.name == c.name).count() != 1
        || c.has_evaluated_accessor()
        || c.static_accessor_names.iter().any(|n| n == key)
    {
        return None;
    }
    let list = if set { &c.setters } else { &c.getters };
    let mut own = list.iter().filter(|(k, _)| k == key);
    let (_, f) = own.next()?;
    if own.next().is_some() || f.id != func {
        return None;
    }
    let member = format!("{}{}", if set { "__set_" } else { "__get_" }, f.name);
    Some(perry_codegen::symbol_names::method(
        prefix, &c.name, &member,
    ))
}

/// ONCHANGE3: what a proven site calls in the holder's module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Member {
    Get,
    Set,
    Method,
}

/// The holder's symbol for a proven site (`Member::Get` / `Set`:
/// `accessor_symbol_in`; `Method`: `method_symbol_in`).
pub(crate) type HolderSymbol<'a> =
    dyn Fn(&str, u32, u32, &str, Member) -> Option<(String, perry_codegen::DirectAbi)> + 'a;

/// ONCHANGE3: the public entry of class `class_id`'s method `key` in the
/// module that defines it, as that module's codegen names it
/// (`method_registry.rs`: `symbol_names::method(module prefix, class name,
/// method name)`; importers declare the same name, method_registry.rs:188),
/// with the ABI its definition has (`DirectAbi`). `None` unless the module
/// holds one class with that id and name whose last method `key` (the one
/// codegen registers) is the body `func`, an ordinary method (not a
/// generator, not async, no fixed generator state), and the class has no
/// evaluated accessor and no static accessor of that name.
pub(crate) fn method_symbol_in(
    hir: &perry_hir::Module,
    prefix: &str,
    class_id: u32,
    func: u32,
    key: &str,
) -> Option<(String, perry_codegen::DirectAbi)> {
    let (c, m) = perry_hir::method_call::target(hir, class_id, func, key)?;
    let abi = perry_codegen::DirectAbi {
        params: m.params.len() as u32,
        rest: m.params.iter().any(|p| p.is_rest),
        synthetic_arguments: m
            .params
            .last()
            .is_some_and(|p| p.arguments_object.is_some()),
        user_rest: m
            .params
            .iter()
            .any(|p| p.is_rest && p.arguments_object.is_none()),
    };
    Some((
        perry_codegen::symbol_names::method(prefix, &c.name, key),
        abi,
    ))
}

/// The view module `path`'s codegen receives.
pub(crate) fn view_for(
    pf: &Option<Arc<perry_wpfacts::ProgramFacts>>,
    path: &Path,
    holder_symbol: &HolderSymbol<'_>,
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
    let elision = elision_for(pf, &key, holder_symbol);
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
fn elision_for(
    pf: &perry_wpfacts::ProgramFacts,
    path: &str,
    holder_symbol: &HolderSymbol<'_>,
) -> perry_codegen::ElisionFacts {
    use perry_codegen::{ElisionFacts, FactsBody, FactsClass, FactsField, FactsRecv};
    use perry_wpfacts::model::prim;
    let mut out = ElisionFacts {
        contracts: contracts_requested(),
        ..Default::default()
    };
    // The certificate is the decision; the driver only names its entry.
    // Keep this before early returns so disabled worlds are asserted too.
    if let Some(module) = pf.module_paths.iter().position(|p| p == path) {
        for s in pf
            .sites
            .iter()
            .filter(|s| s.module as usize == module && s.is_call)
        {
            if let Some(c) = s.method_call_certificate() {
                if let Some((symbol, abi)) = pf
                    .module_paths
                    .get(c.module as usize)
                    .and_then(|p| holder_symbol(p, c.class, c.function, &s.key, Member::Method))
                {
                    out.method_calls.insert(
                        (s.byte_offset, s.key.clone()),
                        (
                            c.class,
                            c.function,
                            symbol,
                            abi,
                            c.nullable,
                            c.direct_imported_instance(),
                        ),
                    );
                }
            }
        }
        for s in pf
            .sites
            .iter()
            .filter(|s| s.module as usize == module && s.is_call)
        {
            debug_assert_eq!(
                s.method_call_certificate().is_some(),
                out.method_calls
                    .contains_key(&(s.byte_offset, s.key.clone())),
                "method-call certificate/publication disagreement at {path}:{} .{}",
                s.byte_offset,
                s.key
            );
        }
    }
    if !elide_enabled() || pf.stats.aborted {
        return out;
    }
    // `PERRY_FACTS_SABOTAGE=unchecked`: the negative control of decision 150
    // (every guarded or checked proof is emitted unchecked; the dynamic-code
    // gap tests must then fail).
    let unchecked = pf.sabotage.as_deref() == Some("unchecked");
    out.interp_guard = pf.interp_guard.is_some() && !unchecked;
    // VR-2: the admitted value records' classes, for check mode
    // only (`expr/record_witness.rs`); empty with the switch off.
    out.record_classes = pf
        .templates
        .iter()
        .filter(|t| t.record.is_ok())
        .filter_map(|t| t.class_id)
        .collect();
    out.super_unrelinked = pf.super_unrelinked.clone();
    out.zero_sign_tolerated = pf.zero_sign_tolerated > 0;
    out.family_never = pf.family_reach.as_ref().map_or(0, |f| f.never());
    out.indexed_metadata_pristine = pf.indexed_metadata_pristine();
    // GUARDS1 class 3. `PERRY_FACTS_SABOTAGE=typed-metadata-unwritten`:
    // claim it for every program (the gap tests that redefine a typed-array
    // prototype's `length` must fail).
    out.typed_metadata_unwritten = pf.sabotage.as_deref() == Some("typed-metadata-unwritten")
        || (!pf.stats.aborted
            && pf.interp_guard.is_none()
            && pf.typed_metadata_written.is_none());
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
            out.native_member_calls =
                perry_wpfacts::native_member_calls_of(pf, module, namespace_pairs_on());
            debug_assert!(
                pf.sites
                    .iter()
                    .filter(|s| s.module as usize == module)
                    .all(|s| !s.builtin_dispatch_certified()
                        || perry_hir::namespace_pairs::intrinsic_selected(
                            s.byte_offset,
                            &out.native_member_calls
                        )),
                "builtin dispatch certificate requires the emitted intrinsic arm"
            );
            // ACCBUILD2 P3: the named sites proven to call one accessor.
            // Every record at an offset must agree, or the offset is left out.
            let mut refused: std::collections::BTreeSet<(
                u32,
                String,
                perry_codegen::AccessorKind,
            )> = Default::default();
            for s in pf
                .sites
                .iter()
                .filter(|s| s.module as usize == module && !s.is_call)
            {
                // PutValue's record is the store; a compound's GetValue
                // (`UpdateGet`, at the store's offset) reads the getter.
                let set = matches!(s.access, perry_wpfacts::derive::Access::Set { .. });
                // The one accessor body (`target_fns`) names the holder's
                // module; its symbol comes from that module's class.
                // COVER3: a receiver that can be `undefined` / `null` beside
                // the objects keeps its receiver test (`nullable`): the
                // nullish base takes the complete operation, which throws.
                // Any other primitive receiver is no direct call.
                let nullish = s.receiver_prims & (prim::NULL | prim::UNDEF) != 0;
                let other_prim = s.receiver_prims & !(prim::NULL | prim::UNDEF) != 0;
                let holder = match (&s.verdict, set, s.holder_class, s.target_fns.as_slice()) {
                    _ if other_prim => None,
                    (perry_wpfacts::Verdict::AccessorSet, true, Some(cid), &[(m, f)])
                    | (perry_wpfacts::Verdict::AccessorGet, false, Some(cid), &[(m, f)]) => pf
                        .module_paths
                        .get(m as usize)
                        .and_then(|p| {
                            let member = if set { Member::Set } else { Member::Get };
                            holder_symbol(p, cid, f, &s.key, member)
                        })
                        .map(|(sym, _)| (cid, sym, nullish)),
                    _ => None,
                };
                // A site is (module, start, kind): a compound's GetValue
                // is never an ordinary read's record, nor the reverse.
                let kind = match s.access {
                    perry_wpfacts::derive::Access::Set { .. } => perry_codegen::AccessorKind::Set,
                    perry_wpfacts::derive::Access::UpdateGet => {
                        perry_codegen::AccessorKind::UpdateGet
                    }
                    perry_wpfacts::derive::Access::Read => perry_codegen::AccessorKind::Get,
                };
                let map = &mut out.accessors;
                let at = (s.byte_offset, s.key.clone(), kind);
                match holder {
                    Some(h) if !refused.contains(&at) => {
                        if *map.entry(at.clone()).or_insert_with(|| h.clone()) != h {
                            map.remove(&at);
                            refused.insert(at);
                        }
                    }
                    _ => {
                        map.remove(&at);
                        refused.insert(at);
                    }
                }
            }
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
        out.inbounds_reads.extend(mb.inbounds_reads.iter().copied());
        out.rest_reads.extend(mb.rest_reads.iter().copied());
        out.rest_writes.extend(mb.rest_writes.iter().copied());
        out.rest_births.extend(mb.rest_births.iter().copied());
        out.rest_reserves.extend(mb.rest_reserves.iter().copied());
        out.list_capacities
            .extend(mb.list_capacities.iter().copied());
        out.rest_pushes.extend(mb.rest_pushes.iter().copied());
        out.fill_push_recvs
            .extend(mb.fill_push_recvs.iter().copied());
        out.fill_births.extend(mb.fill_births.iter().copied());
        out.rest_lane_reads
            .extend(mb.rest_lane_reads.iter().copied());
        // VR-4b: the record arrays. Their record's layout is
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
            // VR-3b: a Record slot whose layout this module
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
        // Same-module typed calls only.
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
    // Method sites (wpfacts `method_sites`): no typed-array receiver fact
    // with `PERRY_FACTS_TYPED_ARRAY_SITES=0`, no Array-member fact with
    // `PERRY_FACTS_ARRAY_METHOD_SITES=0`, none when interpreted code can
    // run. Two records under one key hold a fact only when both do.
    if !interp {
        for (site, key, flags) in &mb.method_sites {
            out.method_sites
                .entry((*site, key.clone()))
                .and_modify(|f| *f &= *flags)
                .or_insert(*flags);
        }
    }
    // Quiet sites (wpfacts `quiet_sites`): empty with
    // `PERRY_FACTS_QUIET_SITES=0`, and when interpreted code can run.
    let quiet = !interp && perry_wpfacts::quiet_sites_enabled();
    if quiet {
        out.quiet_reads.extend(mb.quiet_reads.iter().cloned());
        out.quiet_elements.extend(mb.quiet_elements.iter().copied());
        out.primitive_reads
            .extend(mb.primitive_reads.iter().cloned());
        out.primitive_elements
            .extend(mb.primitive_elements.iter().copied());
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
            // VR-3a: a binding of one admitted value record is
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
            if b.booleans {
                out.boolean_locals.insert(*id);
            }
            if b.nullish_booleans {
                out.nullish_boolean_locals.insert(*id);
            }
            if b.stable_length {
                out.stable_length_locals.insert(*id, b.stable_front);
            }
            if b.named_length {
                out.named_length_locals.insert(*id);
            }
            // A primitive and no object at all: an operator converts
            // nothing of it (`perry_codegen` `quiet`).
            // The negative control `quiet` claims it for every local.
            if quiet
                && (!b.other && b.class_ids.is_empty() && b.prims != 0
                    || pf.sabotage.as_deref() == Some("quiet"))
            {
                out.primitive_locals.insert(*id);
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
        if b.generator_source {
            out.generator_sources.insert(*id);
        }
        if let Some(cid) = b.class_object.filter(|_| !interp) {
            out.local_classes.insert(*id, cid);
        }
    }
    // ONCHG2: the named method calls proven to call one of a closed set of
    // compiled closures / declarations of this module (`T0Mono` / `T0Set`
    // whose `target_bodies` is the set): the value the member read gives is
    // one of them (13.3.6.2), so the call runs that body with the function
    // object as its environment and the receiver as `this` (10.2.1). The
    // bodies of every record at an offset are joined (each record bounds its
    // own evaluations); a record proving less leaves the offset out. A
    // receiver that may be a primitive, `undefined`, `null` or top keeps the
    // site (no nullish arm here). Interpreted code may write any member
    // (decision 150): as the method calls, only without the guard word.
    if let Some(module) = pf
        .module_paths
        .iter()
        .position(|p| p == path)
        .filter(|_| !interp)
    {
        let mut refused: std::collections::BTreeSet<(u32, String)> = Default::default();
        for s in pf
            .sites
            .iter()
            .filter(|s| s.module as usize == module && s.is_call)
        {
            let at = (s.byte_offset, s.key.clone());
            let fs = match s.verdict {
                perry_wpfacts::Verdict::T0Mono | perry_wpfacts::Verdict::T0Set
                    if s.receiver_prims == 0 && !s.top =>
                {
                    bodies(&s.target_bodies)
                }
                _ => Vec::new(),
            };
            if fs.is_empty() || refused.contains(&at) {
                out.late_call_bodies.remove(&at);
                refused.insert(at);
                continue;
            }
            let e = out.late_call_bodies.entry(at).or_default();
            e.extend(fs);
            e.sort();
            e.dedup();
        }
    }
    // The method calls closed over declared methods; a record proving less
    // leaves its offset out.
    if let Some(module) = pf
        .module_paths
        .iter()
        .position(|p| p == path)
        .filter(|_| !interp)
    {
        let mut refused: std::collections::BTreeSet<(u32, String)> = Default::default();
        for s in pf
            .sites
            .iter()
            .filter(|s| s.module as usize == module && s.is_call)
        {
            let at = (s.byte_offset, s.key.clone());
            if s.closed_methods && !refused.contains(&at) {
                out.closed_methods.insert(at);
            } else {
                out.closed_methods.remove(&at);
                refused.insert(at);
            }
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
                // Interpreted code can store anything.
                if f.booleans && !interp {
                    out.boolean_fields.insert((cid, f.key.clone()));
                }
                if f.nullish_booleans && !interp {
                    out.nullish_boolean_fields.insert((cid, f.key.clone()));
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
                // Interpreted code can add any key to any object.
                absent: if interp {
                    Vec::new()
                } else {
                    t.fields
                        .iter()
                        .filter(|f| f.absent && !f.private)
                        .map(|f| f.key.clone())
                        .collect()
                },
            },
        );
    }
    out
}

/// MATHC1: the namespace pair offsets of module `path` whose generic call is
/// proven to read the original builtin ([`perry_wpfacts::native_member_calls_of`]). Empty
/// without the facts, past their bounds, with the checks kept
/// (`PERRY_FACTS_ELIDE=0`) or where interpreted code can run: every pair is
/// then its generic call.
pub(crate) fn namespace_pair_proven(
    pf: &Option<Arc<perry_wpfacts::ProgramFacts>>,
    path: &Path,
) -> BTreeSet<u32> {
    let Some(pf) = pf else {
        return BTreeSet::new();
    };
    let key = path.display().to_string();
    if !elide_enabled() || pf.stats.aborted || pf.interp_guard.is_some() {
        return BTreeSet::new();
    }
    match pf.module_paths.iter().position(|p| *p == key) {
        Some(module) => perry_wpfacts::native_member_calls_of(pf, module, namespace_pairs_on()),
        None => BTreeSet::new(),
    }
}

/// MATHC5: the value calls of module `path` (call byte offset to member `m`)
/// whose callee the solve proves to be exactly the original `Math.<m>`
/// (`Verdict::IntrinsicCall`: the callee set is that one builtin function;
/// `IntrinsicCallee::Exact`: no source left a value out, no unknown code
/// holds the realm, no write reaches the member) and whose native the realm
/// declares `this = ignored` (its algorithm never reads `this`). Each such
/// call takes the intrinsic of `Math.<m>(args…)` (`namespace_pairs`). Empty
/// under the same conditions as [`namespace_pair_proven`].
pub(crate) fn alias_intrinsic_calls(
    pf: &Option<Arc<perry_wpfacts::ProgramFacts>>,
    path: &Path,
) -> BTreeMap<u32, String> {
    use perry_wpfacts::derive::IntrinsicCallee;
    let Some(pf) = pf else {
        return BTreeMap::new();
    };
    if !elide_enabled() || pf.stats.aborted || pf.interp_guard.is_some() {
        return BTreeMap::new();
    }
    let key = path.display().to_string();
    let Some(module) = pf.module_paths.iter().position(|p| *p == key) else {
        return BTreeMap::new();
    };
    let mut out: BTreeMap<u32, Option<String>> = BTreeMap::new();
    for s in pf
        .sites
        .iter()
        .filter(|s| s.module as usize == module && s.is_call && s.key == "()")
    {
        let member = match (&s.verdict, &s.intrinsic_callee) {
            (perry_wpfacts::Verdict::IntrinsicCall, Some(IntrinsicCallee::Exact(name)))
                if perry_wpfacts::native_effects::this_ignored(name) =>
            {
                name.strip_prefix("Math.").map(str::to_owned)
            }
            _ => None,
        };
        // Every site at the offset must agree (a site per function copy).
        let slot = out.entry(s.byte_offset).or_insert_with(|| member.clone());
        if *slot != member {
            *slot = None;
        }
    }
    out.into_iter().filter_map(|(o, m)| Some((o, m?))).collect()
}

/// MATHC3: the function ids of module `path` whose bodies the program never
/// runs (`BodyReach::inactive`: the solve completed, activation classified
/// the body and no path reaches it; the predicate the dead-body removal
/// reads). A namespace pair in one takes its intrinsic arm, the HIR a build
/// without the pair has. Empty under the same conditions as
/// [`namespace_pair_proven`] and when reach is incomplete.
pub(crate) fn namespace_pair_inactive(
    pf: &Option<Arc<perry_wpfacts::ProgramFacts>>,
    path: &Path,
) -> BTreeSet<u32> {
    let Some(pf) = pf else {
        return BTreeSet::new();
    };
    if !elide_enabled() || pf.stats.aborted || pf.interp_guard.is_some() {
        return BTreeSet::new();
    }
    let key = path.display().to_string();
    let reach = &pf.body_reach;
    reach
        .seen
        .iter()
        .filter(|b| b.0 == key && b.1 != u32::MAX && b.2.is_empty() && reach.inactive(b))
        .map(|b| b.1)
        .collect()
}

/// MATHC1: with the whole-program facts on every
/// direct builtin namespace call is lowered as the pair whose one arm the
/// call site's verdict picks; `PERRY_NS_PAIRS=0` keeps the old lowering.
/// GUARDS3: per class id, what a program write may give its instances or
/// its prototype (wpfacts `shadow_writes`), for codegen's prototype-census
/// binding sites: a chain none of them reaches needs no shadow-bit test.
/// `None` (every site keeps its tests) without whole-program facts, with
/// interpreted code, or with elision off. `PERRY_FACTS_SABOTAGE=
/// shadow-unwritten` claims every class the solver saw unwritten.
pub(crate) fn shadow_writes(
    pf: Option<&perry_wpfacts::ProgramFacts>,
) -> Option<std::collections::BTreeMap<u32, perry_codegen::ClassShadowWrites>> {
    let pf = pf?;
    if !elide_enabled() || pf.stats.aborted {
        return None;
    }
    let sabotage = pf.sabotage.as_deref() == Some("shadow-unwritten");
    if !sabotage && pf.interp_guard.is_some() {
        return None;
    }
    Some(
        pf.shadow_writes
            .iter()
            .map(|(&cid, w)| {
                let w = match sabotage {
                    true => perry_codegen::ClassShadowWrites::default(),
                    false => perry_codegen::ClassShadowWrites {
                        inst_names: w.inst_names.clone(),
                        inst_any: w.inst_any.clone(),
                        proto_names: w.proto_names.clone(),
                        proto_any: w.proto_any.clone(),
                        proto_relinked: w.proto_relinked.clone(),
                    },
                };
                (cid, w)
            })
            .collect(),
    )
}

/// GUARDS1 class 2: the program-wide array chain facts codegen's inline
/// element and method guards read (`perry_codegen::
/// set_program_array_chain_facts`): `.0` the default chain's index bit
/// needs no test (DEC 250 locks index adds and relinks of
/// `%Array.prototype%`/`%Object.prototype%`; a relinked array carries its own
/// `GC_ARRAY_CUSTOM_PROTO` bit, which stays tested), claimed for a
/// whole-program facts build; `.1` neither prototype is ever modified
/// (wpfacts `array_methods_written`, no interpreted code), so the
/// `ARRAY_METHODS_MODIFIED` bit needs no test either.
/// `PERRY_FACTS_SABOTAGE=array-chain-unwritten` claims both.
pub(crate) fn array_chain_facts(pf: Option<&perry_wpfacts::ProgramFacts>) -> (bool, bool) {
    let Some(pf) = pf else {
        return (false, false);
    };
    if pf.sabotage.as_deref() == Some("array-chain-unwritten") {
        return (true, true);
    }
    let index = !pf.stats.aborted;
    let methods = index && pf.interp_guard.is_none() && pf.array_methods_written.is_none();
    (index, methods)
}

pub(crate) fn namespace_pairs_on() -> bool {
    perry_wpfacts::enabled_from_env() && std::env::var("PERRY_NS_PAIRS").as_deref() != Ok("0")
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
mod count_tests {
    use super::parse_count;

    #[test]
    fn a_bound_parses_digits_and_scientific_notation_or_is_refused() {
        assert_eq!(parse_count("1000000000"), Some(1_000_000_000));
        assert_eq!(parse_count("1_000_000_000"), Some(1_000_000_000));
        assert_eq!(parse_count("1e10"), Some(10_000_000_000));
        assert_eq!(parse_count("2.5E9"), Some(2_500_000_000));
        assert_eq!(parse_count("1e+3"), Some(1000));
        assert_eq!(parse_count("0"), Some(0));
        assert_eq!(parse_count("1.50e1"), Some(15));
        assert_eq!(parse_count("1.5e0"), None);
        assert_eq!(parse_count("1e-3"), None);
        assert_eq!(parse_count("-5"), None);
        assert_eq!(parse_count("1e30"), None);
        assert_eq!(parse_count("ten"), None);
        assert_eq!(parse_count("e9"), None);
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
#[path = "program_facts/outlined_tests.rs"]
mod outlined_tests;

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
    data_url_import: bool,
    dynamic_code_reach: &perry_hir::dynamic_code::DynamicCodeReach,
    roots: Vec<String>,
) -> Option<perry_wpfacts::Activation> {
    if !perry_wpfacts::enabled_from_env() {
        return None;
    }
    let cfg = perry_wpfacts::FactsConfig {
        namespace_pairs: namespace_pairs_on(),
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
        // Off unless `=1` until VR-6.
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
        optimistic_json_key: std::env::var("PERRY_FACTS_OPTIMISTIC_JSON_KEY").as_deref() == Ok("1"),
        json_positions: std::env::var("PERRY_FACTS_JSON_POSITIONS").as_deref() == Ok("1"),
        sabotage: std::env::var("PERRY_FACTS_SABOTAGE")
            .ok()
            .filter(|s| !s.is_empty()),
        profile: std::env::var("PERRY_FACTS_PROFILE").is_ok_and(|v| v == "1"),
        budget_secs: env_count("PERRY_FACTS_BUDGET_SECS").filter(|&s| s != 0),
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
        data_url_import,
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
            if site.access.is_store() {
                return None;
            }
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
                perry_wpfacts::Verdict::Throws => SiteVerdict::Throws,
                perry_wpfacts::Verdict::NsExport => SiteVerdict::NsExport,
                perry_wpfacts::Verdict::IntrinsicCall => SiteVerdict::IntrinsicCall,
                perry_wpfacts::Verdict::AccessorGet
                | perry_wpfacts::Verdict::AccessorSet
                | perry_wpfacts::Verdict::AddsKey
                | perry_wpfacts::Verdict::KnownObjectStore
                | perry_wpfacts::Verdict::PerKind
                | perry_wpfacts::Verdict::ProxyTrap
                | perry_wpfacts::Verdict::No(_) => return None,
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
/// tier (`internals/representation.md` §7, P4).
fn element_reads_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_WP_ELEMENT_READS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The layout codegen lowers an admitted value record of template `tmpl`
/// with, when its literal is born in module `path` (VR-3a).
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
        // A record's lanes reach codegen apart from this rep (`record_locals`,
        // `record_layouts`, the signatures' record slots); here it is Boxed.
        Rep::Record(_) => FactsRep::Boxed,
    }
}

#[cfg(test)]
#[path = "program_facts/method_call_tests.rs"]
mod method_call_tests;

#[cfg(test)]
#[path = "program_facts/accessor_kind_tests.rs"]
mod accessor_kind_tests;
