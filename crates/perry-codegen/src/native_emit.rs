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
//! Paths not yet ported (codegen-unit splitting, `emit_ir_only`) fall through
//! to the text pipeline, where `native`/`diff` still select the in-process
//! *transport*, so no clang subprocess appears in any in-process mode.
//!
//! Construction consumes `LlFunction::for_each_final_line` — the finalized
//! per-line stream including entry-alloca hoists, boundary splices and
//! return-site rewrites, shared with `to_ir` so those transforms have
//! exactly one implementation. No per-function text is materialized on this
//! path; the exception is `has_try` functions, whose setjmp volatile pass
//! needs whole-function analysis and therefore still renders text. What
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
    let mut instructions = 0usize;
    for f in &funcs {
        let header = synth_define_header(f);
        let mut stream = crate::dialect::FnStream::begin(context, &module, &header)
            .map_err(|e| anyhow!("native IR construction failed in @{}: {:#}", f.name, e))?;
        if f.has_try {
            // The setjmp volatile pass needs whole-function analysis; keep
            // the materialized-text path for try-containing functions.
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
            // The common case: finalized lines stream straight into the
            // C-API builder — no per-function text exists.
            f.for_each_final_line::<anyhow::Error>(&mut |line| stream.line(line))
                .map_err(|e| anyhow!("native IR construction failed in @{}: {:#}", f.name, e))?;
        }
        instructions += stream
            .finish()
            .map_err(|e| anyhow!("native IR construction failed in @{}: {:#}", f.name, e))?;
    }
    log::debug!(
        "perry-codegen: native construction built {} functions, {} instructions, skeleton {} bytes",
        funcs.len(),
        instructions,
        skeleton.len()
    );
    Ok(module)
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

pub fn compile_module_native(llmod: &LlModule, target: Option<&str>) -> Result<Vec<u8>> {
    let context = Context::create();
    let module = build_native_module(&context, llmod)?;
    let (effective_target, args) = plan_for(llmod, target);
    crate::inprocess::optimize_and_emit_module(&module, &effective_target, &args)
}

/// Differential harness: text-parsed arm vs natively-built arm, same LLVM,
/// same plan. The verdict is **emitted object bytes** — the C-API builder
/// constant-folds at construction (`zext i1 false`, `select i1 false, ...`),
/// so pre-optimization prints legitimately differ in a way that vanishes
/// under the pass pipeline; byte-compared objects are the ground truth (the
/// same methodology that proved the Phase 0 transport byte-identical).
/// Returns the text arm's object (the trusted reference) so a diff run is
/// safe for real builds while surfacing every divergence.
pub fn compile_module_diff(llmod: &LlModule, target: Option<&str>) -> Result<Vec<u8>> {
    let text = llmod.to_ir();
    let ctx_text = Context::create();
    let m_text = crate::inprocess::parse_ir_text(&ctx_text, &text, "perry_diff_text")?;
    let (effective_target, args) = plan_for(llmod, target);

    let ctx_native = Context::create();
    let native = build_native_module(&ctx_native, llmod);
    match native {
        Err(e) => {
            eprintln!("perry: [ir-diff] native construction FAILED (text arm still used): {e:#}");
            crate::inprocess::optimize_and_emit_module(&m_text, &effective_target, &args)
        }
        Ok(m_native) => {
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
            Ok(bytes_text)
        }
    }
}

/// The one-line define header for `f`, matching `LlFunction::to_ir`'s
/// rendering (linkage, return type, params, inline/try attributes) so the
/// reader applies identical linkage and function attributes.
fn synth_define_header(f: &crate::function::LlFunction) -> String {
    let params = f
        .params
        .iter()
        .map(|(t, n)| format!("{t} {n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let linkage = if f.linkage.is_empty() {
        String::new()
    } else {
        format!("{} ", f.linkage)
    };
    let attrs = if f.has_try {
        " #1"
    } else if f.force_inline {
        " alwaysinline"
    } else if f.inline_hint {
        " inlinehint"
    } else {
        ""
    };
    format!(
        "define {}{} @{}({}){} {{",
        linkage, f.return_type, f.name, params, attrs
    )
}
