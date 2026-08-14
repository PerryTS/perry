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

use anyhow::{anyhow, Result};
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
    let (typed_insts, raw_insts) = stream_functions(context, &module, &funcs, false)?;
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
            let fn_text = f.to_ir();
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
    estimated_bytes: usize,
    max_function_bytes: usize,
}

fn freeze_unit(
    part: crate::module::OwnedCodegenUnitPart,
    external_declarations: &[(String, String)],
) -> Result<FrozenUnit> {
    let crate::module::OwnedCodegenUnitPart { pre, post, funcs } = part;
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
    let mut estimated_bytes = skeleton.len();
    let mut max_function_bytes = 0usize;
    let mut functions = Vec::with_capacity(function_count);
    for f in funcs {
        let function_bytes = f.estimated_ir_bytes();
        estimated_bytes += function_bytes;
        max_function_bytes = max_function_bytes.max(function_bytes);
        if f.personality.is_some() {
            // Windows SEH funclets (`catchswitch`/`catchpad`/`catchret`) have
            // no inkwell builders. Let LLVM's in-process assembly parser build
            // only these exceptional functions; all ordinary bodies remain on
            // the typed C-API path and never become text.
            skeleton.push_str(&crate::module::render_fn_external(&f));
            skeleton.push('\n');
            continue;
        }
        skeleton.push_str(&crate::module::declare_line_for(&f));
        skeleton.push('\n');
        let mut items = Vec::new();
        if f.stack_map_requested() {
            // `to_ir` is where precise roots are lowered. Freeze its body as
            // owned lines so worker threads still receive an immutable payload
            // and the module-scale text graph is never retained.
            items.extend(
                f.to_ir()
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
            header: synth_define_header(&f, true),
            items,
        });
    }
    Ok(FrozenUnit {
        skeleton,
        functions,
        function_count,
        estimated_bytes,
        max_function_bytes,
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
            match item {
                FrozenItem::Label(s) => stream.item(&FI::Label(s))?,
                FrozenItem::Blank => stream.item(&FI::Blank)?,
                FrozenItem::Text(s) => stream.item(&FI::Text(s))?,
                FrozenItem::Inst(i) => stream.item(&FI::Inst(i))?,
            }
        }
        let (t, r) = stream.finish()?;
        typed += t;
        raw += r;
    }
    Ok((typed, raw))
}

/// Native construction for a module large enough to split into codegen
/// units (#5391): each unit is its own context+module (peak RSS stays
/// ~whole/n, same bound as the per-unit clang model), functions stream with
/// external linkage forced (mirror of `render_fn_external`), and the unit
/// objects partial-link exactly like the text path.
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
    let parts = owned_module.into_codegen_unit_parts(n);
    let show_progress = matches!(
        std::env::var("PERRY_CODEGEN_PROGRESS").as_deref(),
        Ok("1" | "all")
    ) || std::env::var("PERRY_CODEGEN_UNIT_TIMINGS").is_ok();
    let unit_total = parts.len();
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
    let compile_one = |i: usize, unit: &FrozenUnit| -> Result<Vec<u8>> {
        let started = std::time::Instant::now();
        let context = Context::create();
        let module =
            crate::inprocess::parse_ir_text(&context, &unit.skeleton, "perry_native_module")
                .map_err(|e| anyhow!("unit {i} skeleton: {e:#}"))?;
        let (t, r) = stream_frozen_functions(&context, &module, &unit.functions)
            .map_err(|e| anyhow!("unit {i}: {e:#}"))?;
        debug_dump(&module, &format!("{module_prefix}.unit{i}"));
        let (effective_target, args) = crate::linker::native_plan_args(
            target,
            unit.estimated_bytes,
            unit.function_count,
            unit.max_function_bytes,
        );
        let unit_bytes =
            crate::inprocess::optimize_and_emit_module(&module, &effective_target, &args)
                .map_err(|e| anyhow!("unit {i}: {e:#}"))?;
        let obj = crate::linker::finish_native_emission(unit_bytes, &effective_target, &args)
            .map_err(|e| anyhow!("unit {i}: {e:#}"))?;
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
        .unwrap_or(2)
        .min(parts.len());
    if show_progress {
        let estimated_mib: f64 = parts
            .iter()
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
    let completed = std::sync::atomic::AtomicUsize::new(0);
    let frozen = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<Result<Vec<u8>>>>> = (0..parts.len())
        .map(|_| std::sync::Mutex::new(None))
        .collect();
    // The producer alone touches lowering-owned LlFunction/Rc state. Each
    // completed owned payload immediately enters a bounded queue, letting LLVM
    // consume it while the producer freezes later units. Previously all units
    // were frozen into a Vec first: full Claude waited ~5 minutes before LLVM
    // started and retained both graphs at peak RSS.
    let (sender, receiver) =
        std::sync::mpsc::sync_channel::<(usize, Result<FrozenUnit>)>(jobs.max(1));
    let receiver = std::sync::Mutex::new(receiver);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| loop {
                let received = receiver
                    .lock()
                    .expect("native freeze queue poisoned")
                    .recv();
                let Ok((i, frozen_unit)) = received else { break };
                let unit_started = std::time::Instant::now();
                let out = frozen_unit.and_then(|unit| compile_one(i, &unit));
                let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if show_progress {
                    let elapsed = llvm_started.elapsed().as_secs_f64();
                    let eta = if done < unit_total {
                        elapsed / done as f64 * (unit_total - done) as f64
                    } else {
                        0.0
                    };
                    eprintln!(
                        "[perry] codegen: {module_prefix}: LLVM unit {}/{} finished ({:.1}s; {} complete; elapsed {:.1} min; ETA ~{:.1} min)",
                        i + 1, unit_total, unit_started.elapsed().as_secs_f64(), done,
                        elapsed / 60.0, eta / 60.0
                    );
                }
                *slots[i].lock().expect("native codegen-unit slot poisoned") = Some(out);
            });
        }
        let freeze_started = std::time::Instant::now();
        let report_step = (unit_total / 20).max(1);
        // Consume each part as soon as its owned worker payload has been
        // produced. Keeping `parts` alive through the scoped worker join held
        // every unit's large pre/post strings until all LLVM work completed;
        // dropping that multi-gigabyte graph afterwards added a several-minute
        // single-threaded destructor tail on the full Claude Code bundle.
        for (i, part) in parts.into_iter().enumerate() {
            let unit = freeze_unit(part, &external_declarations);
            if sender.send((i, unit)).is_err() {
                break;
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
        }
        drop(sender);
    });
    let mut objs = Vec::with_capacity(unit_total);
    for (i, slot) in slots.into_iter().enumerate() {
        objs.push(
            slot.into_inner()
                .expect("native codegen-unit slot poisoned")
                .expect("every native codegen unit is compiled")
                .map_err(|e| {
                    anyhow!("native codegen unit {}/{} failed: {e:#}", i + 1, unit_total)
                })?,
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
    let units = llmod.render_codegen_units(n);
    let bytes_text = crate::linker::compile_units_to_object(&units, target)?;
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
                    units.len()
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

/// The plan argv for a natively-built module. Same decision code as the text
/// path (`build_clang_compile_plan`), with the byte-size input taken from the
/// render-free size estimate the codegen-unit balancer already uses.
fn plan_for(llmod: &LlModule, target: Option<&str>) -> (String, Vec<String>) {
    let funcs = llmod.deduped_function_refs();
    let est_bytes: usize = funcs.iter().map(|f| f.estimated_ir_bytes()).sum();
    let max_fn_bytes = funcs
        .iter()
        .map(|f| f.estimated_ir_bytes())
        .max()
        .unwrap_or(0);
    crate::linker::native_plan_args(target, est_bytes, funcs.len(), max_fn_bytes)
}

pub fn compile_module_native(
    llmod: &LlModule,
    target: Option<&str>,
    module_prefix: &str,
) -> Result<Vec<u8>> {
    let context = Context::create();
    let module = build_native_module(&context, llmod)?;
    debug_dump(&module, module_prefix);
    let (effective_target, args) = plan_for(llmod, target);
    // #7982: under the statepoint backends the plan asks for `-S`, so this
    // returns assembler TEXT. It must go through the compact-map rewrite and
    // the assembler before it can be called an object — the textual path has
    // always done this, the native path silently did not, and the link died
    // with `ld: unknown file type`.
    let bytes = crate::inprocess::optimize_and_emit_module(&module, &effective_target, &args)?;
    crate::linker::finish_native_emission(bytes, &effective_target, &args)
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
    use crate::types::{I32, I64, PTR, VOID};

    fn precise_root_fixture(extra_plain_function: bool) -> LlModule {
        let mut module = LlModule::new(crate::codegen::default_target_triple());
        module.declare_function("js_shadow_slot_bind", VOID, &[I32, PTR]);
        module.declare_function("js_map_alloc", I64, &[I32]);

        let function = module.define_function("native_root_diff_fixture", I64, vec![]);
        function.enable_shadow_frame(0);
        let root_index = function
            .reserve_shadow_slot()
            .expect("native root fixture reserves one precise-root slot");
        let root = function.alloca_entry(I64);
        function.entry_allocas_push_store(I64, "0", &root);
        function.entry_setup_call_void(
            "js_shadow_slot_bind",
            &[(I32, &root_index.to_string()), (PTR, &root)],
        );
        let entry = function.create_block("entry");
        let value = entry.call(I64, "js_map_alloc", &[(I32, "0")]);
        entry.store(I64, &value, &root);
        entry.ret(I64, &value);
        if extra_plain_function {
            let plain = module.define_function("native_root_diff_plain", VOID, vec![]);
            plain.create_block("entry").ret_void();
        }
        module
    }

    #[test]
    fn native_construction_lowers_precise_roots_before_rs4gc() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let module = precise_root_fixture(false);

        let text_ir = module.to_ir();
        assert!(
            text_ir.contains("alloca ptr addrspace(1)"),
            "control arm must demonstrably lower a precise root:\n{text_ir}"
        );
        assert!(
            !text_ir.contains("call void @js_shadow_slot_bind"),
            "native-root lowering must consume the shadow-stack bind:\n{text_ir}"
        );

        let text = crate::linker::compile_ll_to_object(&text_ir, None)
            .expect("trusted text arm emits an object");
        let native = compile_module_native(&module, None, "native_root_diff_fixture")
            .expect("direct native arm emits an object");
        assert_eq!(
            native, text,
            "a mapped function must be byte-identical after both arms run RS4GC; \
             a behavior-only check is vacuous until a collection"
        );
    }

    #[test]
    fn split_native_construction_lowers_precise_roots_before_rs4gc() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let text_module = precise_root_fixture(true);
        let units = text_module.render_codegen_units(2);
        assert_eq!(units.len(), 2, "fixture must exercise two real units");
        let text = crate::linker::compile_units_to_object(&units, None)
            .expect("trusted text units emit and partial-link");

        let mut native_module = precise_root_fixture(true);
        let native = compile_module_units_native(
            &mut native_module,
            2,
            None,
            "split_native_root_diff_fixture",
        )
        .expect("direct native units emit and partial-link");
        assert_eq!(
            native, text,
            "split native units must freeze finalized precise-root IR, not \
             pre-lowered shadow-slot calls"
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
    llmod: &LlModule,
    target: Option<&str>,
    module_prefix: &str,
) -> Result<Vec<u8>> {
    let text = llmod.to_ir();
    let ctx_text = Context::create();
    let m_text = crate::inprocess::parse_ir_text(&ctx_text, &text, "perry_native_module")?;
    let (effective_target, args) = plan_for(llmod, target);

    let ctx_native = Context::create();
    let native = build_native_module(&ctx_native, llmod);
    match native {
        Err(e) => {
            eprintln!("perry: [ir-diff] native construction FAILED (text arm still used): {e:#}");
            let bytes =
                crate::inprocess::optimize_and_emit_module(&m_text, &effective_target, &args)?;
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
            let bytes_native =
                crate::inprocess::optimize_and_emit_module(&m_native, &effective_target, &args)?;
            let bytes_text =
                crate::inprocess::optimize_and_emit_module(&m_text, &effective_target, &args)?;
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
