//! Automatic specialization for a strictly proven whole-program subset.
//! This does not use the runtime-feature detector as an effects proof.
mod analysis;
mod emit;

use std::fs;
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::{CompilationContext, CompileArgs, CompileResult, JavaScriptPlatform};
use crate::OutputFormat;

pub(super) fn try_compile(
    args: &CompileArgs,
    ctx: &CompilationContext,
    format: OutputFormat,
    verbose: u8,
) -> Result<Option<CompileResult>> {
    if let Err(reason) = build_eligibility(args, ctx) {
        explain_fallback(reason, format, verbose);
        return Ok(None);
    }
    // Reparse the ORIGINAL source, never HIR or a folded/reachable-only body.
    // Normal collection/preflight has already validated configuration and the
    // graph. A full syntax allowlist prevents dead functions/imports/effects
    // from disappearing before this proof can see them.
    let source = fs::read_to_string(&args.input)?;
    let proof = match analysis::analyze(&source, &args.input.to_string_lossy()) {
        Ok(proof) => proof,
        Err(reason) => {
            explain_fallback(reason, format, verbose);
            return Ok(None);
        }
    };
    let ir = emit::llvm_ir(&proof);
    let object = perry_codegen::linker::compile_ll_to_object(&ir, None)
        .context("compiling proven tiny-program entry")?;
    let staging = tempfile::tempdir().context("staging tiny-program object")?;
    let object_path = staging.path().join("tiny.o");
    fs::write(&object_path, object)?;
    let stem = args
        .input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let output = args.output.clone().unwrap_or_else(|| {
        super::output_path::default_output_path(false, false, args.target.as_deref(), stem)
    });
    // Use an absolute output argument so a user filename beginning with '-'
    // cannot be interpreted as a linker option.
    let absolute_output = if output.is_absolute() {
        output.clone()
    } else {
        std::env::current_dir()?.join(&output)
    };
    let mut command = Command::new("cc");
    command.arg(&object_path).arg("-o").arg(&absolute_output);
    if cfg!(target_os = "macos") {
        command.arg("-Wl,-dead_strip");
    } else {
        command.arg("-Wl,--gc-sections");
    }
    if verbose > 0 && matches!(format, OutputFormat::Text) {
        println!(
            "  using proven tiny program: {} constant output calls; no managed runtime",
            proof.outputs().len()
        );
        println!("  tiny link: {command:?}");
    }
    let linked = command
        .output()
        .context("linking tiny program with the host C toolchain")?;
    if !linked.status.success() {
        bail!(
            "tiny-program link failed:\n{}{}",
            String::from_utf8_lossy(&linked.stdout),
            String::from_utf8_lossy(&linked.stderr)
        );
    }
    if args.keep_intermediates {
        fs::copy(&object_path, output.with_extension("tiny.o"))?;
        fs::write(output.with_extension("tiny.ll"), &ir)?;
    }
    super::post_link::strip_final_binary(
        ctx,
        &output,
        args.target.as_deref(),
        false,
        false,
        false,
        false,
        false,
        false,
    );
    super::post_link::print_binary_size(format, &output);
    super::size_report::emit_size_report(format, &output, args.report_size);
    if matches!(format, OutputFormat::Json) {
        println!(
            "{}",
            serde_json::json!({"status":"success", "output":output, "runtimeProfile":"tiny", "outputCalls":proof.outputs().len()})
        );
    }
    Ok(Some(CompileResult {
        output_path: output,
        target: args.target.clone().unwrap_or_else(|| "native".to_string()),
        bundle_id: None,
        is_dylib: false,
        codegen_cache_stats: None,
        link_cache_stats: None,
        build_cache_stats: None,
    }))
}

fn explain_fallback(reason: &str, format: OutputFormat, verbose: u8) {
    if verbose > 0 && matches!(format, OutputFormat::Text) {
        println!("  tiny program fallback: {reason}");
    }
}

fn build_eligibility(args: &CompileArgs, ctx: &CompilationContext) -> Result<(), &'static str> {
    let native_target = args.target.as_deref().is_none_or(|target| {
        target == "native"
            || (cfg!(target_os = "macos") && target == "macos")
            || (cfg!(target_os = "linux") && target == "linux")
    });
    if !cfg!(all(
        target_pointer_width = "64",
        any(target_os = "macos", target_os = "linux")
    )) || !native_target
        || args.libc.is_some()
        || args.output_type != "executable"
        || args.platform != JavaScriptPlatform::Node
    {
        return Err("requires a native Linux/macOS standalone Node-compatible executable");
    }
    // Honor the existing general optimization opt-out; there is no new flag
    // or environment switch to enable this specialization.
    if args.no_auto_optimize || std::env::var_os("PERRY_NO_AUTO_OPTIMIZE").is_some() {
        return Err("automatic optimization disabled");
    }
    if args.no_link
        || args.type_check
        || args.print_hir
        || args.trace.is_some()
        || args.focus.is_some()
        || args.debug_symbols
        || args.opt_report.is_some()
        || args.statepoint_report.is_some()
        || args.explain_lowering
        || args.verify_native_regions
        || args.typed_feedback_profile.is_some()
        || args.typed_feedback_sites.is_some()
    {
        return Err("requested build diagnostics require the normal pipeline");
    }
    if args.enable_wasm_runtime
        || args.bundle_extensions.is_some()
        || args.app_bundle_id.is_some()
        || !args.embed.is_empty()
        || !args.asset_module.is_empty()
        || args.bunfs_root.is_some()
        || args.features.is_some()
        || args.minimal_stdlib
        || args.enable_geisterhand
        || args.geisterhand_port.is_some()
        || ctx.needs_ui
        || ctx.needs_plugins
        || ctx.needs_geisterhand
        || ctx.needs_wasm_runtime
        || ctx.precompile_capture
        || !ctx.native_libraries.is_empty()
        || !ctx.embedded_assets.is_empty()
        || !ctx.define.is_empty()
        || ctx.emit_attest
        || ctx.emit_sandbox
        || ctx.lockdown
    {
        return Err("host features or source transforms require the normal pipeline");
    }
    if ctx.native_modules.len() != 1
        || !ctx.js_modules.is_empty()
        || !ctx.native_module_imports.is_empty()
        || !ctx.native_addons.is_empty()
        || ctx.needs_thread
        || ctx.uses_diagnostics
    {
        return Err("program is not a standalone effect-free source graph");
    }
    // These existing compiler diagnostics require ordinary generated helpers.
    // Merely retaining symbols (PERRY_KEEP_SYMBOLS) is safe and remains useful
    // for checking that the specialized link truly omits the runtime.
    for name in [
        "PERRY_DEBUG_SYMBOLS",
        "PERRY_OPT_REPORT",
        "PERRY_SAVE_LL",
        "PERRY_LLVM_KEEP_IR",
        "PERRY_NATIVEINST_DIAG",
        "PERRY_SEGVIEW_DIAG",
        "PERRY_OUTLINE_ENTRY_REPORT",
    ] {
        if std::env::var_os(name).is_some() {
            return Err("compiler instrumentation requires the normal pipeline");
        }
    }
    Ok(())
}
