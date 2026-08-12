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
        if f.personality.is_some() {
            // The invoke-EH phi-predecessor rewrite (#7302) needs
            // whole-function analysis and therefore text; the line reader
            // DOES understand invoke/landingpad, so this path constructs
            // natively from the rewritten text rather than falling back to
            // clang.
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

/// Native construction for a module large enough to split into codegen
/// units (#5391): each unit is its own context+module (peak RSS stays
/// ~whole/n, same bound as the per-unit clang model), functions stream with
/// external linkage forced (mirror of `render_fn_external`), and the unit
/// objects partial-link exactly like the text path.
pub fn compile_module_units_native(
    llmod: &LlModule,
    n: usize,
    target: Option<&str>,
    module_prefix: &str,
) -> Result<Vec<u8>> {
    let parts = llmod.codegen_unit_parts(n);
    if parts.len() == 1 {
        return compile_module_native(llmod, target, module_prefix);
    }
    let mut objs = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        let context = Context::create();
        let mut skeleton = format!("{}{}", part.pre, part.post);
        for f in &part.funcs {
            skeleton.push_str(&crate::module::declare_line_for(f));
            skeleton.push('\n');
        }
        let module = crate::inprocess::parse_ir_text(&context, &skeleton, "perry_native_module")
            .map_err(|e| anyhow!("unit {i} skeleton: {e:#}"))?;
        let (t, r) = stream_functions(&context, &module, &part.funcs, true)
            .map_err(|e| anyhow!("unit {i}: {e:#}"))?;
        debug_dump(&module, &format!("{module_prefix}.unit{i}"));
        let est: usize = part
            .funcs
            .iter()
            .map(|f| f.estimated_ir_bytes())
            .sum::<usize>()
            + skeleton.len();
        let (effective_target, args) =
            crate::linker::native_plan_args(target, est, part.funcs.len());
        let unit_bytes =
            crate::inprocess::optimize_and_emit_module(&module, &effective_target, &args)
                .map_err(|e| anyhow!("unit {i}: {e:#}"))?;
        objs.push(
            crate::linker::finish_native_emission(unit_bytes, &effective_target, &args)
                .map_err(|e| anyhow!("unit {i}: {e:#}"))?,
        );
        log::debug!(
            "perry-codegen: native unit {i}: {} fns, {t} typed + {r} raw insts",
            part.funcs.len()
        );
    }
    crate::linker::merge_unit_objects(&objs)
}

/// Unit-split differential harness: text-rendered units through the
/// in-process transport vs natively-constructed units, merged objects
/// byte-compared. Returns the text arm (the trusted reference).
pub fn compile_module_units_diff(
    llmod: &LlModule,
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
    let est_bytes: usize = llmod
        .deduped_function_refs()
        .iter()
        .map(|f| f.estimated_ir_bytes())
        .sum();
    let fn_count = llmod.deduped_function_refs().len();
    crate::linker::native_plan_args(target, est_bytes, fn_count)
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
