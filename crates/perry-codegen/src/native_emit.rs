//! Phase 2 of exp/llvm-inprocess: **native module construction** — function
//! bodies are built in memory through the LLVM C API instead of being
//! concatenated into module-scale IR text and re-parsed.
//!
//! Mode selection reuses the `PERRY_LLVM_INPROCESS` env var (already part of
//! both cache keys):
//!
//! * `1`/`on`/`true` — transport mode: whole-module text parsed in-process
//!   (`inprocess.rs`).
//! * `native` — this module: only the module *skeleton* (globals, declares,
//!   attribute groups, metadata — a few KB) is textual; every function body
//!   is constructed natively from the finalized per-function line stream.
//! * `diff` — the migration harness: builds the module BOTH ways in the same
//!   LLVM, prints both, diffs the normalized prints per function, and returns
//!   the text-parsed arm's object. Any non-cosmetic difference is a
//!   construction bug — report it, never normalize it away silently.
//!
//! Codegen-unit splitting is ported (`compile_module_units_native`): each
//! unit is its own context+module, functions stream with external linkage
//! forced, and unit objects partial-link exactly like the text path. The
//! only remaining text fallthrough is `emit_ir_only` (bitcode-link mode),
//! which by definition WANTS the whole-module text.
//!
//! Construction consumes `LlFunction::for_each_final_line` — the finalized
//! per-line stream including entry-alloca hoists, boundary splices and
//! return-site rewrites, shared with `to_ir` so those transforms have
//! exactly one implementation. No per-function text is materialized on this
//! path. (Until #7302 `has_try` functions were an exception, because the
//! setjmp volatile pass needed whole-function analysis and forced a text
//! render; invoke/landingpad deleted that pass, so no such exception
//! remains.) What
//! stops existing everywhere is the module-scale concatenation and the
//! full-grammar LLVM parse. The follow-up (typed `LlInst` variants) removes
//! the remaining per-LINE formatting; the `instructions=` counter logged per
//! module is that migration's ratchet.

use anyhow::{anyhow, Context as _, Result};
use inkwell::context::Context;
use inkwell::module::Module;

use crate::module::LlModule;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeMode {
    Off,
    Native,
    Diff,
}

pub fn native_mode() -> NativeMode {
    match std::env::var("PERRY_LLVM_INPROCESS").as_deref() {
        Ok("native") => NativeMode::Native,
        Ok("diff") => NativeMode::Diff,
        _ => NativeMode::Off,
    }
}

/// Large split modules default to native construction: this is where serially
/// rendering module-scale IR is catastrophic and where independent LLVM
/// contexts provide useful parallelism. Small/single-unit modules retain the
/// mature text transport unless `=native` is explicit.
pub fn native_units_mode() -> NativeMode {
    match std::env::var("PERRY_LLVM_INPROCESS").as_deref() {
        Ok("0" | "off" | "false" | "1" | "on" | "true") => NativeMode::Off,
        Ok("diff") => NativeMode::Diff,
        Ok("native") | Err(_) => NativeMode::Native,
        Ok(_) => NativeMode::Off,
    }
}

/// Build the module natively: parse the skeleton text, then construct every
/// function body through the C API via the dialect reader.
fn build_native_module<'ctx>(context: &'ctx Context, llmod: &LlModule) -> Result<Module<'ctx>> {
    let mut skeleton = llmod.skeleton_ir();
    let funcs = llmod.deduped_function_refs();
    // Append a declare for every define: the skeleton's globals can
    // reference defined functions (extern-closure descriptors hold wrapper
    // function addresses), and calls to functions defined later in the
    // module are module-scope forward references. Parsing declares with the
    // define's real signature covers both; `declare_from_header` upgrades
    // linkage when the body is read.
    for f in &funcs {
        let tys = f
            .params
            .iter()
            .map(|(t, _)| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        skeleton.push_str(&format!("declare {} @{}({})\n", f.return_type, f.name, tys));
    }
    let module = crate::inprocess::parse_ir_text(context, &skeleton, "perry_native_module")?;
    let gc_leaf_callees = crate::gc_call_effects::transitive_leaf_functions(&funcs);
    let (typed_insts, raw_insts) =
        stream_functions(context, &module, &funcs, false, &gc_leaf_callees)?;
    log::debug!(
        "perry-codegen: native construction built {} functions, {} typed + {} raw instructions \
         (ratchet: raw -> 0), skeleton {} bytes",
        funcs.len(),
        typed_insts,
        raw_insts,
        skeleton.len()
    );
    Ok(module)
}

/// Stream every function's finalized items into the module. Returns
/// `(typed, raw)` instruction totals — the migration ratchet.
fn stream_functions<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    funcs: &[&crate::function::LlFunction],
    force_external: bool,
    gc_leaf_callees: &std::collections::HashSet<String>,
) -> Result<(usize, usize)> {
    let mut typed_insts = 0usize;
    let mut raw_insts = 0usize;
    for f in funcs {
        let header = synth_define_header(f, force_external);
        let mut stream = crate::dialect::FnStream::begin(context, module, &header)
            .map_err(|e| anyhow!("native IR construction failed in @{}: {:#}", f.name, e))?;
        if f.personality.is_some() || f.stack_map_requested() {
            // Invoke-EH phi predecessors and precise-root lowering both need
            // whole-function analysis. The latter turns shadow-slot binds
            // into addrspace(1) roots before RS4GC; streaming the pre-lowered
            // FinalItems would produce a verifier-clean module with no roots.
            // This remains native construction: only one finalized function
            // is materialized and fed through the closed dialect line reader,
            // never parsed as module-scale IR.
            let fn_text = f.to_ir_with_gc_leaf_callees(gc_leaf_callees);
            for line in fn_text.lines().skip(1) {
                stream.line(line).map_err(|e| {
                    anyhow!(
                        "native IR construction failed in @{}:\n{}\n--- function IR ---\n{}",
                        f.name,
                        e,
                        fn_text
                    )
                })?;
            }
        } else {
            // The common case: finalized items stream straight into the
            // C-API builder — typed instructions carry no text at all.
            f.for_each_final_item::<anyhow::Error>(&mut |item| stream.item(&item))
                .map_err(|e| anyhow!("native IR construction failed in @{}: {:#}", f.name, e))?;
        }
        let (t, r) = stream
            .finish()
            .map_err(|e| anyhow!("native IR construction failed in @{}: {:#}", f.name, e))?;
        typed_insts += t;
        raw_insts += r;
    }
    Ok((typed_insts, raw_insts))
}

enum FrozenItem {
    Label(String),
    Blank,
    Text(String),
    Inst(crate::inst::LlInst),
}
struct FrozenFunction {
    name: String,
    header: String,
    items: Vec<FrozenItem>,
}
struct FrozenUnit {
    skeleton: String,
    functions: Vec<FrozenFunction>,
    function_count: usize,
}

/// Apply a typed post-RS4GC budget request to the lowering-owned functions
/// that produced a module/unit. The request is expected to make progress for
/// every named function; otherwise retrying would either preserve the refusal
/// or loop forever, so fail with the original names and counts instead.
pub(crate) fn apply_budget_spill_retry<'a>(
    funcs: impl IntoIterator<Item = &'a mut crate::function::LlFunction>,
    violations: &[crate::inprocess::Rs4gcBudgetViolation],
) -> Result<()> {
    let mut changed = std::collections::HashSet::new();
    for function in funcs {
        let Some(violation) = violations
            .iter()
            .find(|violation| function.name == violation.name)
        else {
            continue;
        };
        if function.request_shadow_frame_spill() {
            changed.insert(violation.name.clone());
            eprintln!(
                "perry: `{}` exceeded the post-RS4GC instruction budget ({} -> {} \
                     instructions; cap {}); retrying it with precise GC roots in a shadow \
                     frame at the requested optimization level (#8679)",
                violation.name,
                violation
                    .pre_instructions
                    .map_or_else(|| "unknown".to_string(), |n| n.to_string()),
                violation.post_instructions,
                violation.cap,
            );
        }
    }
    let missing: Vec<&str> = violations
        .iter()
        .filter(|violation| !changed.contains(&violation.name))
        .map(|violation| violation.name.as_str())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "post-RS4GC budget requested a shadow-frame retry for {}, but those \
             functions were not available for a new lowering (or were already retried)",
            missing.join(", ")
        ))
    }
}

fn freeze_unit(
    part: &crate::module::OwnedCodegenUnitPart,
    external_declarations: &[(String, String)],
) -> Result<FrozenUnit> {
    let crate::module::OwnedCodegenUnitPart {
        pre,
        post,
        funcs,
        gc_leaf_callees,
    } = part;
    let mut skeleton = format!("{pre}{post}");
    // Text units minimize declarations with a rendered-reference scan. Typed
    // instructions can name helpers without passing through that textual scan
    // (e.g. shadow-slot maintenance), so native construction uses the complete
    // external table. This is small compared with the bodies we deliberately
    // no longer render. Avoid duplicating declarations already selected into
    // `part.pre`.
    let mut declared: std::collections::HashSet<String> = skeleton
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            (line.starts_with("declare ") || line.starts_with("define ")).then_some(line)
        })
        .filter_map(|line| line.split_once('@').map(|(_, tail)| tail))
        .filter_map(|tail| tail.split_once('(').map(|(name, _)| name.to_string()))
        .collect();
    for (name, line) in external_declarations {
        if declared.insert(name.clone()) {
            skeleton.push_str(line);
            skeleton.push('\n');
        }
    }
    let function_count = funcs.len();
    let mut functions = Vec::with_capacity(function_count);
    for f in funcs {
        if f.personality.is_some() {
            // Windows SEH funclets (`catchswitch`/`catchpad`/`catchret`) have
            // no inkwell builders. Let LLVM's in-process assembly parser build
            // only these exceptional functions; all ordinary bodies remain on
            // the typed C-API path and never become text.
            skeleton.push_str(&crate::module::render_fn_external_with_gc_leaf_callees(
                &f,
                &gc_leaf_callees,
            ));
            skeleton.push('\n');
            continue;
        }
        skeleton.push_str(&crate::module::declare_line_for(f));
        skeleton.push('\n');
        let mut items = Vec::new();
        if f.stack_map_requested() {
            // `to_ir` is where precise roots are lowered. Freeze its body as
            // owned lines so worker threads still receive an immutable payload
            // and the module-scale text graph is never retained.
            items.extend(
                f.to_ir_with_gc_leaf_callees(&gc_leaf_callees)
                    .lines()
                    .skip(1)
                    .filter(|line| *line != "}")
                    .map(|line| FrozenItem::Text(line.to_string())),
            );
        } else {
            f.for_each_final_item::<anyhow::Error>(&mut |item| {
                use crate::function::FinalItem as FI;
                items.push(match item {
                    FI::Label(s) => FrozenItem::Label(s.to_string()),
                    FI::Blank => FrozenItem::Blank,
                    FI::Text(s) => FrozenItem::Text(s.to_string()),
                    FI::Inst(i) => FrozenItem::Inst(i.clone()),
                });
                Ok(())
            })?;
        }
        functions.push(FrozenFunction {
            name: f.name.clone(),
            header: synth_define_header(f, true),
            items,
        });
    }
    Ok(FrozenUnit {
        skeleton,
        functions,
        function_count,
    })
}

fn stream_frozen_functions<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    funcs: &[FrozenFunction],
) -> Result<(usize, usize)> {
    let (mut typed, mut raw) = (0usize, 0usize);
    for f in funcs {
        let mut stream = crate::dialect::FnStream::begin(context, module, &f.header)
            .map_err(|e| anyhow!("native IR construction failed in @{}: {e:#}", f.name))?;
        for item in &f.items {
            use crate::function::FinalItem as FI;
            let res = match item {
                FrozenItem::Label(s) => stream.item(&FI::Label(s)),
                FrozenItem::Blank => stream.item(&FI::Blank),
                FrozenItem::Text(s) => stream.item(&FI::Text(s)),
                FrozenItem::Inst(i) => stream.item(&FI::Inst(i)),
            };
            res.map_err(|e| dump_dialect_failure(f, e))?;
        }
        let (t, r) = stream.finish().map_err(|e| dump_dialect_failure(f, e))?;
        typed += t;
        raw += r;
    }
    Ok((typed, raw))
}

/// Diagnostic for a dialect construction failure (e.g. "register %rN was used
/// but never defined"): name the offending function and, when
/// `PERRY_DIALECT_DUMP=<dir>` is set, write the function's full constructed IR
/// text (typed insts rendered via `render_into`) to `<dir>/<name>.ll` so the
/// malformed use site is visible. The failing unit never parses, so the normal
/// `PERRY_SAVE_LL` post-parse dump cannot capture it.
fn dump_dialect_failure(f: &FrozenFunction, e: anyhow::Error) -> anyhow::Error {
    if let Ok(dir) = std::env::var("PERRY_DIALECT_DUMP") {
        let _ = std::fs::create_dir_all(&dir);
        let mut buf = String::new();
        buf.push_str(&f.header);
        buf.push('\n');
        for item in &f.items {
            match item {
                FrozenItem::Label(s) => {
                    buf.push_str(s);
                    buf.push('\n');
                }
                FrozenItem::Blank => buf.push('\n'),
                FrozenItem::Text(s) => {
                    buf.push_str(s);
                    buf.push('\n');
                }
                FrozenItem::Inst(i) => {
                    i.render_into(&mut buf);
                    buf.push('\n');
                }
            }
        }
        let safe: String = f
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let _ = std::fs::write(format!("{dir}/{safe}.ll"), &buf);
    }
    anyhow!("native IR construction failed in @{}: {e:#}", f.name)
}

/// Native construction for a module large enough to split into codegen
/// units (#5391): each unit is its own context+module (peak RSS stays
/// ~whole/n, same bound as the per-unit clang model), functions stream with
/// external linkage forced (mirror of `render_fn_external`), and the unit
/// objects partial-link exactly like the text path.
/// Default number of concurrent LLVM unit workers when `PERRY_CODEGEN_UNIT_JOBS`
/// is unset.
///
/// This was a hard-coded `2` (#8017), chosen conservatively for Windows
/// pagefile pressure and applied on every platform. On a large real bundle
/// that left most cores idle: the Claude Code `cli.js` lowers to ~84 codegen
/// units and, with the giant entry function's roots spilled (#8583) so no unit
/// carries an unbounded RS4GC fan-out, per-unit peak RSS is a bounded ~1-2 GiB,
/// so the two-worker cap — not memory — was the wall (a ~440s unit × dozens,
/// two at a time, is hours). Each worker still holds one whole translation
/// unit, so the count stays bounded, not one-thread-per-unit.
///
/// Non-Windows: half the machine's logical CPUs, clamped to `[2, 8]`. The 8
/// ceiling keeps peak at ~8 × per-unit against a 64 GiB-class host with margin;
/// projects that know their headroom raise it with `PERRY_CODEGEN_UNIT_JOBS`.
/// Windows keeps the conservative `2` until its pagefile behavior under higher
/// fan-out is measured — the platform the original cap was chosen for.
fn default_unit_workers() -> usize {
    if cfg!(target_os = "windows") {
        return 2;
    }
    std::thread::available_parallelism()
        .map(|p| (p.get() / 2).clamp(2, 8))
        .unwrap_or(2)
}

pub fn compile_module_units_native(
    llmod: &mut LlModule,
    n: usize,
    target: Option<&str>,
    module_prefix: &str,
) -> Result<Vec<u8>> {
    if llmod.deduped_function_refs().len() <= 1 || n <= 1 {
        return compile_module_native(llmod, target, module_prefix);
    }
    let external_declarations: Vec<(String, String)> = llmod
        .declaration_lines()
        .filter(|(name, _)| !llmod.has_function(name))
        .map(|(name, line)| (name.to_string(), line.to_string()))
        .collect();
    let target_triple = llmod.target_triple.clone();
    let owned_module = std::mem::replace(llmod, LlModule::new(target_triple));
    // Keep at most a bounded window of lowering-owned units alive after they
    // are frozen. A post-RS4GC budget miss needs that source graph exactly
    // once so the named functions can switch root lowering and be frozen
    // again; successful units are still dropped immediately (#8679).
    let mut parts: Vec<Option<crate::module::OwnedCodegenUnitPart>> = owned_module
        .into_codegen_unit_parts(n)
        .into_iter()
        .map(Some)
        .collect();
    let unit_timings = std::env::var("PERRY_CODEGEN_UNIT_TIMINGS").is_ok();
    let show_progress = matches!(
        std::env::var("PERRY_CODEGEN_PROGRESS").as_deref(),
        Ok("1" | "all")
    ) || unit_timings;
    let unit_total = parts.len();
    // Root lowering was selected while the module was produced. Preserve that
    // exact backend choice across the worker boundary instead of re-reading
    // fresh thread-local defaults in each LLVM thread (#8070).
    let native_roots = crate::codegen::helpers::native_stack_roots_enabled();
    if show_progress {
        eprintln!(
            "[perry] codegen: {module_prefix}: freezing {unit_total} codegen units for worker threads"
        );
    }
    // Freeze the lowering-owned Rc/RefCell graph before sharing work. Worker
    // threads receive only owned immutable strings and typed instructions.
    // A few locally-defined wrappers are also predeclared during lowering.
    // The complete external table must exclude those names, matching
    // `LlModule::skeleton_ir`; cross-unit declarations with their actual
    // signatures already live in each part's filtered `pre`.
    let llvm_started = std::time::Instant::now();
    #[cfg(test)]
    let test_budget = crate::inprocess::test_rs4gc_budget_cap();
    let compile_one = |i: usize, unit: &FrozenUnit| -> Result<Vec<u8>> {
        let started = std::time::Instant::now();
        let context = Context::create();
        let module =
            crate::inprocess::parse_ir_text(&context, &unit.skeleton, "perry_native_module")
                .with_context(|| format!("unit {i} skeleton"))?;
        let (t, r) = stream_frozen_functions(&context, &module, &unit.functions)
            .with_context(|| format!("unit {i}"))?;
        debug_dump(&module, &format!("{module_prefix}.unit{i}"));
        let (effective_target, args) = crate::linker::native_plan_args(target, native_roots);
        let mut stats = crate::inprocess::UnitCodegenStats::default();
        let stats_out = unit_timings.then_some(&mut stats);
        let optimize = || {
            crate::inprocess::optimize_and_emit_module_with_stats(
                &module,
                &effective_target,
                &args,
                native_roots,
                stats_out,
            )
        };
        #[cfg(test)]
        let optimized = crate::inprocess::with_inherited_test_rs4gc_budget(test_budget, optimize);
        #[cfg(not(test))]
        let optimized = optimize();
        let unit_bytes = optimized.with_context(|| format!("unit {i}"))?;
        if unit_timings {
            let widest = |w: &Option<(String, usize)>| {
                w.as_ref()
                    .map(|(name, n)| format!("{name} {n}"))
                    .unwrap_or_else(|| "-".to_string())
            };
            let growth = if stats.pre_rewrite_instructions > 0 {
                stats.post_rewrite_instructions as f64 / stats.pre_rewrite_instructions as f64
            } else {
                0.0
            };
            eprintln!(
                "[perry] codegen: {module_prefix}: unit {}/{unit_total}: {} fns; pre-RS4GC {} instrs (widest {}); post-RS4GC {} instrs (x{growth:.1}; widest {}); rs4gc {:.1}s, opt {:.1}s, emit {:.1}s",
                i + 1,
                stats.functions,
                stats.pre_rewrite_instructions,
                widest(&stats.pre_rewrite_widest),
                stats.post_rewrite_instructions,
                widest(&stats.post_rewrite_widest),
                stats.rewrite_secs,
                stats.optimize_secs,
                stats.emit_secs,
            );
        }
        let obj = crate::linker::finish_native_emission(unit_bytes, &effective_target, &args)
            .with_context(|| format!("unit {i}"))?;
        log::debug!(
            "perry-codegen: native unit {i}: {} fns, {t} typed + {r} raw insts, {:.3}s",
            unit.function_count,
            started.elapsed().as_secs_f64()
        );
        Ok(obj)
    };

    let jobs = std::env::var("PERRY_CODEGEN_UNIT_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&v| v > 0)
        .unwrap_or_else(default_unit_workers)
        .min(parts.len());
    if show_progress {
        let estimated_mib: f64 = parts
            .iter()
            .flatten()
            .map(|part| {
                (part.pre.len()
                    + part.post.len()
                    + part
                        .funcs
                        .iter()
                        .map(|f| f.estimated_ir_bytes())
                        .sum::<usize>()) as f64
                    / 1_048_576.0
            })
            .sum::<f64>();
        eprintln!(
            "[perry] codegen: {module_prefix}: freeze/LLVM pipeline started: {unit_total} units, {jobs} workers, ~{estimated_mib:.1} MiB estimated IR"
        );
    }
    let frozen = std::sync::atomic::AtomicUsize::new(0);
    let mut slots: Vec<Option<Result<Vec<u8>>>> = (0..parts.len()).map(|_| None).collect();
    // The producer alone touches lowering-owned LlFunction/Rc state. Workers
    // return their result through a second channel; on a typed budget request
    // the producer can mutate that still-local graph, freeze it again, and
    // resubmit it. The in-flight window stays bounded so this retry ability
    // does not restore the old whole-bundle retention peak.
    let (sender, receiver) =
        std::sync::mpsc::sync_channel::<(usize, Result<FrozenUnit>)>(jobs.max(1));
    let (result_sender, result_receiver) =
        std::sync::mpsc::channel::<(usize, std::time::Duration, Result<Vec<u8>>)>();
    let receiver = std::sync::Mutex::new(receiver);
    std::thread::scope(|scope| {
        for worker_index in 0..jobs {
            let result_sender = result_sender.clone();
            let receiver = &receiver;
            let compile_one = &compile_one;
            // LLVM recursion depth scales with function size, and a post-RS4GC
            // relocation-fan-out function reaches millions of instructions
            // (#8082) — Rust's default 2 MiB worker stack SIGBUSes on the
            // guard page mid-pass with no crash report. Reserve a deep stack;
            // it is address space, not resident memory, until touched.
            std::thread::Builder::new()
                .name(format!("perry-llvm-unit-{worker_index}"))
                .stack_size(64 * 1024 * 1024)
                .spawn_scoped(scope, move || loop {
                    let received = receiver
                        .lock()
                        .expect("native freeze queue poisoned")
                        .recv();
                    let Ok((i, frozen_unit)) = received else {
                        break;
                    };
                    let unit_started = std::time::Instant::now();
                    let out = frozen_unit.and_then(|unit| compile_one(i, &unit));
                    if result_sender
                        .send((i, unit_started.elapsed(), out))
                        .is_err()
                    {
                        break;
                    }
                })
                .expect("spawn LLVM unit worker");
        }
        drop(result_sender);
        let freeze_started = std::time::Instant::now();
        let report_step = (unit_total / 20).max(1);
        let enqueue = |i: usize, part: &crate::module::OwnedCodegenUnitPart, retry: bool| -> bool {
            if unit_timings {
                // Name the widest body before LLVM ever sees it: the one
                // irreducible function in a bundle is the one that sets the
                // unit's time and memory, and a stuck unit number alone does
                // not say which (#8583).
                if let Some(widest) = part.funcs.iter().max_by_key(|f| f.estimated_ir_bytes()) {
                    eprintln!(
                        "[perry] codegen: {module_prefix}: {}unit {}/{unit_total}: {} fns, ~{:.1} MiB estimated IR, widest {} (~{:.1} MiB)",
                        if retry { "retry " } else { "" },
                        i + 1,
                        part.funcs.len(),
                        part.funcs.iter().map(|f| f.estimated_ir_bytes()).sum::<usize>() as f64 / 1_048_576.0,
                        widest.name,
                        widest.estimated_ir_bytes() as f64 / 1_048_576.0
                    );
                }
            }
            let unit = freeze_unit(part, &external_declarations);
            if sender.send((i, unit)).is_err() {
                return false;
            }
            if retry {
                return true;
            }
            let done = frozen.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if show_progress && (done == unit_total || done % report_step == 0) {
                let elapsed = freeze_started.elapsed().as_secs_f64();
                let eta = elapsed * unit_total.saturating_sub(done) as f64 / done as f64;
                eprintln!(
                    "[perry] codegen: {module_prefix}: froze {done}/{unit_total} units ({:.0}%; {:.1}s elapsed; ETA ~{:.1}s)",
                    done as f64 * 100.0 / unit_total as f64,
                    elapsed,
                    eta
                );
            }
            true
        };

        // One source unit per worker. Retrying requires retaining that source
        // until LLVM answers, but there is no reason to retain a second queued
        // source per worker too; freezing the next unit after one completes is
        // only a small producer step and keeps the extra peak tightly bounded.
        let max_in_flight = jobs.clamp(1, unit_total);
        let mut next = 0usize;
        let mut in_flight = 0usize;
        while next < max_in_flight {
            let part = parts[next]
                .as_ref()
                .expect("an undispatched native unit still owns its lowering graph");
            if !enqueue(next, part, false) {
                break;
            }
            next += 1;
            in_flight += 1;
        }

        let mut done = 0usize;
        while done < unit_total && in_flight != 0 {
            let Ok((i, attempt_elapsed, out)) = result_receiver.recv() else {
                break;
            };
            if let Err(error) = &out {
                if let Some(violations) = crate::inprocess::rs4gc_budget_retry(error) {
                    let retry = parts[i]
                        .as_mut()
                        .expect("a retryable native unit keeps its lowering graph");
                    match apply_budget_spill_retry(retry.funcs.iter_mut(), &violations) {
                        Ok(()) if enqueue(i, retry, true) => continue,
                        Ok(()) => {
                            slots[i] = Some(Err(anyhow!(
                                "native codegen retry queue closed for unit {}/{}",
                                i + 1,
                                unit_total
                            )));
                        }
                        Err(retry_error) => {
                            slots[i] = Some(Err(retry_error.context(format!(
                                "native codegen unit {}/{} could not honor its RS4GC budget retry: \
                                 {error:#}",
                                i + 1,
                                unit_total
                            ))));
                        }
                    }
                } else {
                    slots[i] = Some(out);
                }
            } else {
                slots[i] = Some(out);
            }

            // A final result no longer needs its Rc/RefCell lowering graph.
            // Drop it now, not after every unit and LLVM worker has finished.
            parts[i].take();
            done += 1;
            in_flight -= 1;
            if show_progress {
                let elapsed = llvm_started.elapsed().as_secs_f64();
                let eta = if done < unit_total {
                    elapsed / done as f64 * (unit_total - done) as f64
                } else {
                    0.0
                };
                eprintln!(
                    "[perry] codegen: {module_prefix}: LLVM unit {}/{} finished ({:.1}s; {} complete; elapsed {:.1} min; ETA ~{:.1} min)",
                    i + 1,
                    unit_total,
                    attempt_elapsed.as_secs_f64(),
                    done,
                    elapsed / 60.0,
                    eta / 60.0
                );
            }

            if next < unit_total {
                let part = parts[next]
                    .as_ref()
                    .expect("an undispatched native unit still owns its lowering graph");
                if enqueue(next, part, false) {
                    next += 1;
                    in_flight += 1;
                }
            }
        }
        drop(sender);
    });
    let mut objs = Vec::with_capacity(unit_total);
    for (i, slot) in slots.into_iter().enumerate() {
        objs.push(
            slot.expect("every native codegen unit is compiled")
                .with_context(|| format!("native codegen unit {}/{} failed", i + 1, unit_total))?,
        );
    }
    let merge_started = std::time::Instant::now();
    if show_progress {
        let object_mib = objs.iter().map(Vec::len).sum::<usize>() as f64 / 1_048_576.0;
        eprintln!(
            "[perry] codegen: {module_prefix}: merging {unit_total} unit objects (~{object_mib:.1} MiB) into one linker input"
        );
    }
    let merged = crate::linker::merge_unit_objects(&objs);
    if show_progress {
        match &merged {
            Ok(bytes) => eprintln!(
                "[perry] codegen: {module_prefix}: merged {unit_total} unit objects into {:.1} MiB in {:.1}s",
                bytes.len() as f64 / 1_048_576.0,
                merge_started.elapsed().as_secs_f64()
            ),
            Err(_) => eprintln!(
                "[perry] codegen: {module_prefix}: merging {unit_total} unit objects failed after {:.1}s",
                merge_started.elapsed().as_secs_f64()
            ),
        }
    }
    merged
}

/// Unit-split differential harness: text-rendered units through the
/// in-process transport vs natively-constructed units, merged objects
/// byte-compared. Returns the text arm (the trusted reference).
pub fn compile_module_units_diff(
    llmod: &mut LlModule,
    n: usize,
    target: Option<&str>,
    module_prefix: &str,
) -> Result<Vec<u8>> {
    let (bytes_text, text_unit_count) = loop {
        let units = llmod.render_codegen_units(n);
        match crate::linker::compile_units_to_object(&units, target) {
            Ok(bytes) => break (bytes, units.len()),
            Err(error) => {
                let Some(violations) = crate::inprocess::rs4gc_budget_retry(&error) else {
                    return Err(error);
                };
                apply_budget_spill_retry(llmod.functions_mut(), &violations)?;
            }
        }
    };
    match compile_module_units_native(llmod, n, target, module_prefix) {
        Err(e) => {
            eprintln!("perry: [ir-diff] native unit construction FAILED (text arm used): {e:#}");
        }
        Ok(bytes_native) => {
            if bytes_text == bytes_native {
                eprintln!(
                    "perry: [ir-diff] OK — native and text unit arms emit byte-identical merged \
                    objects ({} bytes, {} units)",
                    bytes_text.len(),
                    text_unit_count
                );
            } else {
                eprintln!(
                    "perry: [ir-diff] MISMATCH — merged unit objects differ (text {} vs native {})",
                    bytes_text.len(),
                    bytes_native.len()
                );
            }
        }
    }
    Ok(bytes_text)
}

/// The plan argv for a natively-built module. Uses the same decision code as
/// the text path (`build_clang_compile_plan`).
fn plan_for(target: Option<&str>, native_roots: bool) -> (String, Vec<String>) {
    crate::linker::native_plan_args(target, native_roots)
}

pub fn compile_module_native(
    llmod: &mut LlModule,
    target: Option<&str>,
    module_prefix: &str,
) -> Result<Vec<u8>> {
    let native_roots = crate::codegen::helpers::native_stack_roots_enabled();
    let (effective_target, args) = plan_for(target, native_roots);
    loop {
        let context = Context::create();
        let module = build_native_module(&context, llmod)?;
        debug_dump(&module, module_prefix);
        // #7982: under the statepoint backends the plan asks for `-S`, so this
        // returns assembler TEXT. It must go through the compact-map rewrite
        // and the assembler before it can be called an object.
        match crate::inprocess::optimize_and_emit_module(
            &module,
            &effective_target,
            &args,
            native_roots,
        ) {
            Ok(bytes) => {
                return crate::linker::finish_native_emission(bytes, &effective_target, &args);
            }
            Err(error) => {
                let Some(violations) = crate::inprocess::rs4gc_budget_retry(&error) else {
                    return Err(error);
                };
                apply_budget_spill_retry(llmod.functions_mut(), &violations)?;
            }
        }
    }
}

/// The debug view under native construction: `PERRY_SAVE_LL=<dir>` (which
/// `--trace llvm` sets, #7154) and `PERRY_LLVM_KEEP_IR` both print the
/// CONSTRUCTED module — exactly what LLVM will verify and optimize,
/// including construction-time constant folds — rather than re-rendering
/// the emitter's text. Filenames mirror the text path's so tooling that
/// greps the trace dir keeps working; the `.native` infix says which
/// pipeline produced them.
fn debug_dump(module: &Module<'_>, module_prefix: &str) {
    let keep = std::env::var_os("PERRY_LLVM_KEEP_IR").is_some();
    let save_dir = std::env::var("PERRY_SAVE_LL").ok();
    if !keep && save_dir.is_none() {
        return;
    }
    let printed = module.print_to_string().to_string();
    if let Some(dir) = save_dir {
        let path = format!("{}/{}.native.ll", dir, module_prefix);
        if std::fs::write(&path, &printed).is_ok() {
            eprintln!("[perry-codegen] saved native-construction IR: {path}");
        }
    }
    if keep {
        let path = std::env::temp_dir().join(format!(
            "perry_native_{}_{}.ll",
            module_prefix,
            std::process::id()
        ));
        if std::fs::write(&path, &printed).is_ok() {
            eprintln!(
                "[perry-codegen] kept LLVM IR (native construction): {}",
                path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::LlModule;

    #[test]
    fn default_unit_workers_are_bounded_and_platform_aware() {
        let n = super::default_unit_workers();
        if cfg!(target_os = "windows") {
            assert_eq!(
                n, 2,
                "Windows keeps the conservative 2-worker default (#8017)"
            );
        } else {
            assert!(
                (2..=8).contains(&n),
                "non-Windows default must stay in [2, 8], got {n}"
            );
        }
    }

    use crate::types::{I1, I32, I64, PTR, VOID};

    fn precise_root_fixture(extra_plain_function: bool) -> LlModule {
        precise_root_fixture_for(
            &crate::codegen::default_target_triple(),
            extra_plain_function,
        )
    }

    fn precise_root_fixture_for(triple: &str, extra_plain_function: bool) -> LlModule {
        let mut module = LlModule::new(triple);
        module.declare_function_with_ret_attrs("js_shadow_frame_enter", PTR, &[I32], "nonnull");
        module.declare_function("js_shadow_frame_pop", VOID, &[I64]);
        module.declare_function("js_shadow_slot_bind", VOID, &[I32, PTR]);
        module.declare_function("js_map_alloc", I64, &[I32]);
        module.declare_function("may_collect", I64, &[]);

        let function = module.define_function("native_root_diff_fixture", I64, vec![]);
        function.enable_shadow_frame(0);
        let mut constant_roots = Vec::new();
        let mut dynamic_roots = Vec::new();
        for roots in [&mut constant_roots, &mut dynamic_roots] {
            for _ in 0..8 {
                let root_index = function
                    .reserve_shadow_slot()
                    .expect("native root fixture reserves a precise-root slot");
                let root = function.alloca_entry(I64);
                function.entry_allocas_push_store(I64, "0", &root);
                function.entry_setup_call_void(
                    "js_shadow_slot_bind",
                    &[(I32, &root_index.to_string()), (PTR, &root)],
                );
                roots.push(root);
            }
        }
        let entry = function.create_block("entry");
        // The C-API builder folds this select while whole-module textual IR
        // retains it until SCCP. Both shapes must converge BEFORE
        // RS4GC decides which SSA roots cross the safepoint (#8065).
        for root in &constant_roots {
            let constant = entry.select(
                I1,
                "false",
                I64,
                "9222246136947933188",
                "9222246136947933185",
            );
            entry.store(I64, &constant, root);
        }
        for root in &dynamic_roots {
            let dynamic = entry.call(I64, "js_map_alloc", &[(I32, "0")]);
            entry.store(I64, &dynamic, root);
        }
        let _safepoint = entry.call(I64, "may_collect", &[]);
        // Both values stay live across may_collect. The dynamic one is the
        // positive witness: pre-RS4GC canonicalization must not erase it.
        let mut observed = entry.load(I64, &dynamic_roots[0]);
        for root in constant_roots.iter().chain(dynamic_roots.iter().skip(1)) {
            let value = entry.load(I64, root);
            observed = entry.xor(I64, &observed, &value);
        }
        entry.ret(I64, &observed);
        if extra_plain_function {
            let plain = module.define_function("native_root_diff_plain", VOID, vec![]);
            plain.create_block("entry").ret_void();
        }
        module
    }

    fn assert_dynamic_root_survives_rs4gc(module: &LlModule, label: &str) {
        let target = crate::codegen::default_target_triple();
        let text_ir = module.to_ir();
        let context = Context::create();
        let native_ir = build_native_module(&context, module)
            .expect("native root witness constructs")
            .print_to_string()
            .to_string();
        for (arm, ir) in [("text", text_ir), ("native", native_ir)] {
            let rewritten = crate::inprocess::statepoint_rewritten_ir(
                &ir,
                &target,
                &format!("{label}_{arm}_root_witness"),
            )
            .unwrap_or_else(|e| panic!("{arm} root witness must run RS4GC: {e:#}"));
            assert!(
                rewritten.contains("\"gc-live\"(ptr addrspace(1)"),
                "{arm} arm lost the positive dynamic root before RS4GC:\n{rewritten}"
            );
            assert!(
                rewritten.contains("gc.relocate"),
                "{arm} arm did not relocate the live dynamic root:\n{rewritten}"
            );
        }
    }

    /// #8583: `optnone` stamped BEFORE `rewrite-statepoints-for-gc` is not a
    /// compile-time escape hatch, it is a rooting bug. The new pass manager
    /// skips `mem2reg`/`sccp` on an `optnone` function while RS4GC (a module
    /// pass keyed on the `gc` attribute) still runs, so the root allocas are
    /// never promoted and the collector never sees them: no `gc-live` operand
    /// bundle, no relocation. This is why the pre-rewrite
    /// `PERRY_LL_PREOPT_OPTNONE_INSTRS` knob was removed rather than
    /// calibrated, and why any future size policy must run AFTER the rewrite.
    #[test]
    fn optnone_before_rs4gc_hides_every_root_from_the_collector() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let module = precise_root_fixture(false);
        let target = crate::codegen::default_target_triple();
        let text_ir = module.to_ir();
        assert!(
            text_ir.contains("gc \"statepoint-example\" {"),
            "fixture must carry the GC strategy:\n{text_ir}"
        );
        // Control: the same fixture without optnone roots and relocates.
        assert_dynamic_root_survives_rs4gc(&module, "optnone_control");

        let demoted = text_ir.replace(
            "gc \"statepoint-example\" {",
            "optnone noinline gc \"statepoint-example\" {",
        );
        let rewritten =
            crate::inprocess::statepoint_rewritten_ir(&demoted, &target, "optnone_before_rs4gc")
                .expect("optnone fixture must still run RS4GC");
        assert!(
            !rewritten.contains("\"gc-live\"(ptr addrspace(1)"),
            "an optnone function kept its roots visible to RS4GC, so the pre-rewrite \
             demotion would be sound after all and this test (and the knob's removal) \
             needs revisiting:\n{rewritten}"
        );
        assert!(
            rewritten.contains("= alloca ptr addrspace(1)"),
            "the root allocas should survive unpromoted under optnone:\n{rewritten}"
        );
    }

    /// #8121, emission half. The sibling pair in `inprocess::tests` proves the
    /// LLVM mechanism (RS4GC breaks an unmarked inline-asm barrier, and
    /// `gc-leaf-function` stops it) using hand-written IR, so it would still
    /// pass if Perry stopped emitting the attribute. This asserts the emission
    /// itself, on both paths.
    #[test]
    fn perry_emits_the_loop_barrier_as_a_gc_leaf() {
        let mut module = LlModule::new(crate::codegen::default_target_triple());
        let function = module.define_function("barrier_emission_fixture", VOID, vec![]);
        let entry = function.create_block("entry");
        entry.asm_sideeffect_barrier();
        entry.ret_void();

        let text_ir = module.to_ir();
        assert!(
            text_ir.contains("asm sideeffect"),
            "fixture emitted no barrier, so this proves nothing:\n{text_ir}"
        );
        assert!(
            text_ir.contains(r#"call void asm sideeffect "", ""() "gc-leaf-function""#),
            "text path barrier lost its gc-leaf callsite attribute (#8121):\n{text_ir}"
        );

        let context = Context::create();
        let native_ir = build_native_module(&context, &module)
            .expect("barrier emission fixture constructs")
            .print_to_string()
            .to_string();
        assert!(
            native_ir.contains("asm sideeffect"),
            "native arm emitted no barrier, so this proves nothing:\n{native_ir}"
        );
        assert!(
            native_ir.contains("gc-leaf-function"),
            "native path lost the gc-leaf attribute on the barrier (#8121):\n{native_ir}"
        );
    }

    /// #8596: whole-module generated-callee effects must reach both emission
    /// transports. The text path spells the string attribute inline; LLVM's
    /// C API prints it through an attribute group. RS4GC is the final arbiter:
    /// both forms must leave `pure_generated` direct and wrap `may_collect`.
    #[test]
    fn transitive_generated_leaf_calls_match_text_and_native_construction() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let mut module = LlModule::new(crate::codegen::default_target_triple());
        module.declare_function("js_shadow_slot_bind", VOID, &[I32, PTR]);
        module.declare_function("may_collect", VOID, &[]);

        let pure = module.define_function("pure_generated", VOID, vec![]);
        pure.create_block("entry").ret_void();

        let caller = module.define_function("rooted_leaf_caller", VOID, vec![]);
        caller.enable_shadow_frame(0);
        let slot = caller.reserve_shadow_slot().expect("reserve native root");
        let root = caller.alloca_entry(I64);
        caller.entry_allocas_push_store(I64, "0", &root);
        caller.entry_setup_call_void(
            "js_shadow_slot_bind",
            &[(I32, &slot.to_string()), (PTR, &root)],
        );
        let entry = caller.create_block("entry");
        entry.call_void("pure_generated", &[]);
        entry.call_void("may_collect", &[]);
        entry.ret_void();

        let text_ir = module.to_ir();
        let context = Context::create();
        let native_ir = build_native_module(&context, &module)
            .expect("native transitive-leaf witness constructs")
            .print_to_string()
            .to_string();
        assert!(
            text_ir.contains("call void @pure_generated() \"gc-leaf-function\""),
            "text path lost transitive leaf marker:\n{text_ir}"
        );
        assert!(
            native_ir.contains("\"gc-leaf-function\""),
            "native path lost transitive leaf marker:\n{native_ir}"
        );
        let units = module.render_codegen_units(2);
        assert_eq!(units.len(), 2, "fixture must split into two real units");
        assert!(
            units
                .iter()
                .any(|unit| unit.contains("call void @pure_generated() \"gc-leaf-function\"")),
            "split text units lost the whole-module leaf closure:\n{}",
            units.join("\n--- unit ---\n")
        );

        let target = crate::codegen::default_target_triple();
        for (arm, ir) in [("text", text_ir), ("native", native_ir)] {
            let rewritten = crate::inprocess::statepoint_rewritten_ir(
                &ir,
                &target,
                &format!("transitive_leaf_{arm}"),
            )
            .unwrap_or_else(|e| panic!("{arm} transitive-leaf witness failed RS4GC: {e:#}"));
            assert!(
                rewritten.contains("call void @pure_generated()"),
                "{arm} path statepointed a proven leaf call:\n{rewritten}"
            );
            assert!(
                rewritten.lines().any(|line| {
                    line.contains("@llvm.experimental.gc.statepoint")
                        && line.contains("@may_collect")
                }),
                "{arm} path failed to statepoint the collecting control:\n{rewritten}"
            );
        }
    }

    fn compact_gc_map_section_name() -> &'static [u8] {
        if cfg!(target_os = "macos") {
            b"__perry_gcmap"
        } else if cfg!(target_os = "windows") {
            b".pgcmap"
        } else {
            b".perry_gcmap"
        }
    }

    fn object_contains(object: &[u8], needle: &[u8]) -> bool {
        object.windows(needle.len()).any(|window| window == needle)
    }

    fn assert_compact_gc_map(object: &[u8], label: &str) {
        let section_name = compact_gc_map_section_name();
        assert!(
            object_contains(object, section_name),
            "{label} object has no compact GC-map section"
        );
        assert!(
            object_contains(object, b"PGCM"),
            "{label} compact GC-map section has no map payload"
        );
    }

    fn assert_no_compact_gc_map(object: &[u8], label: &str) {
        assert!(
            !object_contains(object, compact_gc_map_section_name()),
            "{label} shadow-stack object unexpectedly has a compact GC-map section"
        );
        assert!(
            !object_contains(object, b"PGCM"),
            "{label} shadow-stack object unexpectedly has a compact GC-map payload"
        );
    }

    fn compile_text_units_on_producer(units: &[String]) -> Vec<u8> {
        let objects = units
            .iter()
            .map(|unit| {
                crate::linker::compile_ll_to_object(unit, None)
                    .expect("trusted text unit emits an object")
            })
            .collect::<Vec<_>>();
        crate::linker::merge_unit_objects(&objects).expect("trusted text units partial-link")
    }

    #[test]
    fn native_construction_lowers_precise_roots_before_rs4gc() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let mut module = precise_root_fixture(false);

        let text_ir = module.to_ir();
        assert!(
            text_ir.contains("alloca ptr addrspace(1)"),
            "control arm must demonstrably lower a precise root:\n{text_ir}"
        );
        assert!(
            !text_ir.contains("call void @js_shadow_slot_bind"),
            "native-root lowering must consume the shadow-stack bind:\n{text_ir}"
        );
        assert_dynamic_root_survives_rs4gc(&module, "direct");

        let text = crate::linker::compile_ll_to_object(&text_ir, None)
            .expect("trusted text arm emits an object");
        let native = compile_module_native(&mut module, None, "native_root_diff_fixture")
            .expect("direct native arm emits an object");
        assert_eq!(
            native, text,
            "a mapped function must be byte-identical after both arms run RS4GC; \
             a behavior-only check is vacuous until a collection"
        );
    }

    /// #8679: a real backend budget miss must come back through the native
    /// constructor, mutate the lowering-owned function, rebuild the module,
    /// and finish emission. The one-instruction cap guarantees that the first
    /// RS4GC arm trips without constructing a million-instruction fixture;
    /// the successful result and retained shadow IR prove this is a retry,
    /// not the former hard refusal or a disabled budget.
    #[test]
    fn post_rs4gc_budget_retries_with_a_shadow_frame() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let mut module = precise_root_fixture(false);
        let before = module
            .deduped_function_refs()
            .into_iter()
            .find(|function| function.name == "native_root_diff_fixture")
            .expect("fixture function exists before the retry")
            .to_ir();
        assert!(before.contains("gc \"statepoint-example\""), "{before}");
        assert!(!before.contains("@js_shadow_frame_enter"), "{before}");

        let object = crate::inprocess::with_test_rs4gc_budget(1, || {
            compile_module_native(&mut module, None, "rs4gc_budget_retry_fixture")
        })
        .expect("a post-RS4GC budget miss must spill and retry successfully");
        assert!(!object.is_empty());

        let retried = module
            .deduped_function_refs()
            .into_iter()
            .find(|function| function.name == "native_root_diff_fixture")
            .expect("fixture function survives the retry");
        assert!(retried.spills_roots_to_shadow_frame());
        let after = retried.to_ir();
        assert!(!after.contains("gc \"statepoint-example\""), "{after}");
        assert!(after.contains("@js_shadow_frame_enter"), "{after}");
        assert!(after.contains("@js_shadow_slot_bind"), "{after}");
        assert!(after.contains("@js_shadow_frame_pop"), "{after}");
    }

    /// The reported Claude bundle takes the split-unit worker path. Its retry
    /// source must stay on the producer thread (the `LlFunction` graph is not
    /// `Send`) while LLVM reports the typed violation from a worker. A compact
    /// map would prove the worker silently missed the test cap and kept the
    /// statepoint lowering; no map proves the successful object came from the
    /// resubmitted shadow-frame unit.
    #[test]
    fn split_unit_budget_retry_returns_a_shadow_rooted_object() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let mut module = precise_root_fixture(true);
        let before = module.render_codegen_units(2);
        assert!(
            before
                .iter()
                .any(|unit| unit.contains("gc \"statepoint-example\"")),
            "fixture must initially send a mapped function through RS4GC"
        );

        let object = crate::inprocess::with_test_rs4gc_budget(1, || {
            compile_module_units_native(&mut module, 2, None, "rs4gc_split_budget_retry_fixture")
        })
        .expect("a worker budget miss must be re-lowered and resubmitted");
        assert!(!object.is_empty());
        assert_no_compact_gc_map(&object, "budget-retried split native");
    }

    #[test]
    fn split_native_construction_lowers_precise_roots_before_rs4gc() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let text_module = precise_root_fixture(true);
        assert_dynamic_root_survives_rs4gc(&text_module, "split");
        let units = text_module.render_codegen_units(2);
        assert_eq!(units.len(), 2, "fixture must exercise two real units");
        // Compile the trusted units sequentially on this pinned producer
        // thread. Going through compile_units_to_object here made the control
        // machine-dependent: on a high-core host it spawned workers too, both
        // arms lost the same thread-local decision, and byte equality passed
        // while BOTH objects omitted the map (#8070).
        let text = compile_text_units_on_producer(&units);
        assert_compact_gc_map(&text, "trusted text");

        let mut native_module = precise_root_fixture(true);
        let native = compile_module_units_native(
            &mut native_module,
            2,
            None,
            "split_native_root_diff_fixture",
        )
        .expect("direct native units emit and partial-link");
        assert_compact_gc_map(&native, "split native");
        assert_eq!(
            native, text,
            "split native units must freeze finalized precise-root IR, not \
             pre-lowered shadow-slot calls"
        );
    }

    /// #8087: the same construction-path comparison, pinned to an **ELF**
    /// target rather than the host's.
    ///
    /// The three sibling tests above ran only against the host triple, so on a
    /// macOS developer machine they exercised Mach-O exclusively — and Mach-O
    /// records no `STT_FILE` symbol. That is precisely why a module-name
    /// difference that made all of them fail on the Linux runner was invisible
    /// locally for two days. Naming the object format explicitly keeps this
    /// check honest on every host.
    #[test]
    fn native_and_text_arms_agree_on_an_elf_target() {
        const ELF_TRIPLE: &str = "x86_64-unknown-linux-gnu";
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let mut module = precise_root_fixture_for(ELF_TRIPLE, false);

        let text = crate::linker::compile_ll_to_object(&module.to_ir(), Some(ELF_TRIPLE))
            .expect("trusted text arm emits an ELF object");
        let native =
            compile_module_native(&mut module, Some(ELF_TRIPLE), "native_root_elf_fixture")
                .expect("direct native arm emits an ELF object");

        assert_eq!(
            &text[..4],
            b"\x7fELF",
            "fixture must actually produce ELF, or this test proves nothing"
        );
        assert_eq!(
            native, text,
            "native and text construction must emit byte-identical ELF objects; \
             a difference here is a recorded-name or lowering divergence (#8087)"
        );
    }

    #[test]
    fn split_native_construction_propagates_shadow_backend_to_workers() {
        let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
        let text_module = precise_root_fixture(true);
        let text_ir = text_module.to_ir();
        assert!(
            text_ir.contains("call void @js_shadow_slot_bind"),
            "negative control must demonstrably use the shadow-stack lowering:\n{text_ir}"
        );
        assert!(
            !text_ir.contains("alloca ptr addrspace(1)"),
            "negative control must not contain native-stack root allocas:\n{text_ir}"
        );
        let units = text_module.render_codegen_units(2);
        assert_eq!(units.len(), 2, "fixture must exercise two real units");
        let text = compile_text_units_on_producer(&units);
        assert_no_compact_gc_map(&text, "trusted text");

        let mut native_module = precise_root_fixture(true);
        let native = compile_module_units_native(
            &mut native_module,
            2,
            None,
            "split_shadow_root_diff_fixture",
        )
        .expect("direct shadow-stack native units emit and partial-link");
        assert_no_compact_gc_map(&native, "split native");
        assert_eq!(
            native, text,
            "split native workers must preserve the producer's shadow-stack backend decision"
        );
    }

    #[test]
    fn split_units_emit_and_merge_init_body_pointer_constant() {
        // Production webpack modules are large enough to use split native
        // codegen. Every non-entry module's guard passes `__init_body` to the
        // exception boundary as an i64 constant expression. Keep the callee
        // and wrapper in separate units so this covers declaration lookup,
        // relocatable ptrtoint construction, object emission, and the partial
        // link -- a parse-only test would miss the production-graph failure
        // tracked in #8057.
        let mut module = LlModule::new(crate::codegen::default_target_triple());
        module.declare_function("js_run_module_init_catching", VOID, &[I64]);

        let body = module.define_function("fixture_js__init_body", VOID, vec![]);
        body.create_block("entry").ret_void();

        let wrapper = module.define_function("fixture_js__init", VOID, vec![]);
        let entry = wrapper.create_block("entry");
        entry.call_void(
            "js_run_module_init_catching",
            &[(I64, "ptrtoint (ptr @fixture_js__init_body to i64)")],
        );
        entry.ret_void();

        let object =
            compile_module_units_native(&mut module, 2, None, "split_ptrtoint_init_body_fixture")
                .expect("split native units must emit and partial-link");
        assert!(
            !object.is_empty(),
            "merged object must contain emitted code"
        );
    }
}

/// Differential harness: text-parsed arm vs natively-built arm, same LLVM,
/// same plan. The verdict is **emitted object bytes** — the C-API builder
/// constant-folds at construction (`zext i1 false`, `select i1 false, ...`),
/// so pre-optimization prints legitimately differ in a way that vanishes
/// under the pass pipeline; byte-compared objects are the ground truth (the
/// same methodology that proved the Phase 0 transport byte-identical).
/// Returns the text arm's object (the trusted reference) so a diff run is
/// safe for real builds while surfacing every divergence.
pub fn compile_module_diff(
    llmod: &mut LlModule,
    target: Option<&str>,
    module_prefix: &str,
) -> Result<Vec<u8>> {
    loop {
        match compile_module_diff_once(llmod, target, module_prefix) {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                let Some(violations) = crate::inprocess::rs4gc_budget_retry(&error) else {
                    return Err(error);
                };
                apply_budget_spill_retry(llmod.functions_mut(), &violations)?;
            }
        }
    }
}

fn compile_module_diff_once(
    llmod: &LlModule,
    target: Option<&str>,
    module_prefix: &str,
) -> Result<Vec<u8>> {
    let text = llmod.to_ir();
    let ctx_text = Context::create();
    let m_text = crate::inprocess::parse_ir_text(&ctx_text, &text, "perry_native_module")?;
    let native_roots = crate::codegen::helpers::native_stack_roots_enabled();
    let (effective_target, args) = plan_for(target, native_roots);

    let ctx_native = Context::create();
    let native = build_native_module(&ctx_native, llmod);
    match native {
        Err(e) => {
            eprintln!("perry: [ir-diff] native construction FAILED (text arm still used): {e:#}");
            let bytes = crate::inprocess::optimize_and_emit_module(
                &m_text,
                &effective_target,
                &args,
                native_roots,
            )?;
            crate::linker::finish_native_emission(bytes, &effective_target, &args)
        }
        Ok(m_native) => {
            debug_dump(&m_native, module_prefix);
            // Capture pre-opt prints BEFORE optimization mutates the modules;
            // they are the localization artifact when bytes mismatch.
            let dump_dir = std::env::var("PERRY_LLVM_DIFF_DIR").ok();
            let (pre_text, pre_native) = if dump_dir.is_some() {
                (
                    m_text.print_to_string().to_string(),
                    m_native.print_to_string().to_string(),
                )
            } else {
                (String::new(), String::new())
            };
            let bytes_native = crate::inprocess::optimize_and_emit_module(
                &m_native,
                &effective_target,
                &args,
                native_roots,
            )?;
            let bytes_text = crate::inprocess::optimize_and_emit_module(
                &m_text,
                &effective_target,
                &args,
                native_roots,
            )?;
            if bytes_text == bytes_native {
                eprintln!(
                    "perry: [ir-diff] OK — native and text arms emit byte-identical objects \
                     ({} bytes)",
                    bytes_text.len()
                );
            } else {
                eprintln!(
                    "perry: [ir-diff] MISMATCH — object bytes differ (text {} vs native {}); \
                     set PERRY_LLVM_DIFF_DIR to dump both arms' pre-opt IR",
                    bytes_text.len(),
                    bytes_native.len()
                );
                if let Some(dir) = &dump_dir {
                    let _ = std::fs::create_dir_all(dir);
                    let _ = std::fs::write(format!("{dir}/text_arm.ll"), &pre_text);
                    let _ = std::fs::write(format!("{dir}/native_arm.ll"), &pre_native);
                    let _ = std::fs::write(format!("{dir}/text_arm.o"), &bytes_text);
                    let _ = std::fs::write(format!("{dir}/native_arm.o"), &bytes_native);
                    eprintln!("perry: [ir-diff] arms dumped under {dir}");
                }
            }
            // The verdict above is over the bytes LLVM emitted, which under
            // the statepoint plan (`-S`) are assembly. The RETURNED artifact
            // still has to be an object, or the link dies with `ld: unknown
            // file type` (#7982) — the diff arm shared the native arm's bug
            // and was never reached in CI, because the native arm failed
            // first.
            crate::linker::finish_native_emission(bytes_text, &effective_target, &args)
        }
    }
}

/// The `define ... {` line for `f`, delegated to the single renderer
/// [`crate::function::LlFunction::define_header`].
///
/// **#7982 — this used to be a COPY of `to_ir`'s header, and the copy
/// drifted.** It was written against a `to_ir` that had neither
/// `"frame-pointer"="non-leaf"` nor `gc "statepoint-example"`; both were added
/// to `to_ir` afterwards and never here. Missing the GC strategy means RS4GC
/// never runs on a natively-constructed module: it verifies, links and
/// executes correctly on any program that does not collect, while having **no
/// precise roots at all** — #7332's shape, invisible to a behaviour-parity
/// smoke arm by construction. The only symptom was the diff arm's byte
/// mismatch (149,105 text vs 50,995 native on the spike), and the diff arm was
/// never reached because the native arm failed earlier.
///
/// The fix is structural rather than a test for agreement: there is now one
/// renderer, so the next attribute added to the header reaches both paths.
fn synth_define_header(f: &crate::function::LlFunction, force_external: bool) -> String {
    f.define_header(force_external)
}
