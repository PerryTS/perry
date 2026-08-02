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
/// same printer. Returns the text arm's object (the trusted reference) so a
/// diff run is safe to use for real builds while surfacing every divergence.
pub fn compile_module_diff(llmod: &LlModule, target: Option<&str>) -> Result<Vec<u8>> {
    let text = llmod.to_ir();
    let ctx_text = Context::create();
    let m_text = crate::inprocess::parse_ir_text(&ctx_text, &text, "perry_diff_text")?;

    let ctx_native = Context::create();
    let native = build_native_module(&ctx_native, llmod);

    match native {
        Err(e) => {
            eprintln!("perry: [ir-diff] native construction FAILED (text arm still used): {e:#}");
        }
        Ok(m_native) => {
            let pa = normalize_print(&m_text.print_to_string().to_string());
            let pb = normalize_print(&m_native.print_to_string().to_string());
            if pa == pb {
                eprintln!(
                    "perry: [ir-diff] OK — native construction matches text parse ({} lines)",
                    pa.lines().count()
                );
            } else {
                report_diff(&pa, &pb);
                if let Ok(dir) = std::env::var("PERRY_LLVM_DIFF_DIR") {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(format!("{dir}/text_arm.ll"), &pa);
                    let _ = std::fs::write(format!("{dir}/native_arm.ll"), &pb);
                    eprintln!("perry: [ir-diff] arms dumped under {dir}");
                }
            }
        }
    }

    let (effective_target, args) = plan_for(llmod, target);
    crate::inprocess::optimize_and_emit_module(&m_text, &effective_target, &args)
}

/// Normalization limited to the classes catalogued in
/// `docs/llvm-inprocess-experiment.md` (attribute-group *numbering* and
/// header trivia). Everything else must match exactly — both arms come out
/// of the same LLVM printer.
fn normalize_print(printed: &str) -> String {
    let mut out = String::with_capacity(printed.len());
    for line in printed.lines() {
        if line.starts_with("; ModuleID") || line.starts_with("source_filename") {
            continue;
        }
        // `#12` group ids may be assigned in a different order by the two
        // construction orders; the *contents* of the groups still compare
        // via the `attributes` lines themselves (with ids blanked).
        let mut norm = String::with_capacity(line.len());
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '#' && chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                    chars.next();
                }
                norm.push_str("#N");
            } else {
                norm.push(c);
            }
        }
        out.push_str(&norm);
        out.push('\n');
    }
    out
}

fn report_diff(pa: &str, pb: &str) {
    let a: Vec<&str> = pa.lines().collect();
    let b: Vec<&str> = pb.lines().collect();
    let mut shown = 0;
    let mut i = 0;
    let mut j = 0;
    eprintln!(
        "perry: [ir-diff] MISMATCH — native construction diverges from text parse \
         ({} vs {} lines); first divergences:",
        a.len(),
        b.len()
    );
    // Naive sync-on-equal walk: enough to *locate* divergence; the dumped
    // arms (PERRY_LLVM_DIFF_DIR) are the real investigation artifact.
    while i < a.len() && j < b.len() && shown < 12 {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        eprintln!("  text  : {}", a[i]);
        eprintln!("  native: {}", b[j]);
        shown += 1;
        i += 1;
        j += 1;
    }
}
