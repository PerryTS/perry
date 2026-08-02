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
//! Construction consumes `LlFunction::to_ir()` output per function — the
//! fully finalized text including entry-alloca hoists, boundary splices,
//! return-site rewrites and setjmp volatile promotion — so those transforms
//! have exactly one implementation. The per-function text is transient
//! (dropped immediately after parsing); what stops existing is the
//! module-scale concatenation and the full-grammar LLVM parse. The follow-up
//! step (typed `LlInst` in `LlBlock`) removes the per-line text too; the
//! `instructions=` counter logged per module is the ratchet metric for that
//! migration.

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
    let skeleton = llmod.skeleton_ir();
    let module = crate::inprocess::parse_ir_text(context, &skeleton, "perry_native_module")?;
    let funcs = llmod.deduped_function_refs();
    // Pre-declare every define: calls to functions defined later in the
    // module (and closure address captures) are module-scope forward
    // references. Only the one-line header is synthesized here; bodies are
    // read once, below.
    for f in &funcs {
        let param_str = f
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
        let header = format!(
            "define {}{} @{}({}) {{",
            linkage, f.return_type, f.name, param_str
        );
        crate::dialect::predeclare_function_from_text(context, &module, &header)
            .map_err(|e| anyhow!("pre-declaring @{}: {}", f.name, e))?;
    }
    let mut instructions = 0usize;
    for f in &funcs {
        let fn_text = f.to_ir();
        instructions += crate::dialect::add_function_from_text(context, &module, &fn_text)
            .map_err(|e| {
                anyhow!(
                    "native IR construction failed in @{}:\n{}\n--- function IR ---\n{}",
                    f.name,
                    e,
                    fn_text
                )
            })?;
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
