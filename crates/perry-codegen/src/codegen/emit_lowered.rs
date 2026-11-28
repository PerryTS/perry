//! Emission of a lowered module: IR finalization, then object code through
//! the textual, unit-split or in-process LLVM paths.

use anyhow::Result;

use super::{decide_codegen_units, numeric_remainder, CompileProgress};
use crate::module::LlModule;

pub(super) struct EmissionOptions {
    pub(super) target: Option<String>,
    pub(super) emit_ir_only: bool,
    pub(super) verify_native_regions: bool,
    pub(super) output_type: String,
}

pub(super) struct LoweredModule {
    pub(super) llmod: LlModule,
    pub(super) progress: CompileProgress,
    pub(super) module_name: String,
    pub(super) callable_count: usize,
    pub(super) interned_strings: usize,
    pub(super) opts: EmissionOptions,
    pub(super) module_prefix: String,
    pub(super) triple: String,
}

pub(super) fn emit_lowered_module(lowered: LoweredModule) -> Result<Vec<u8>> {
    let LoweredModule {
        mut llmod,
        progress,
        module_name,
        callable_count,
        interned_strings,
        opts,
        module_prefix,
        triple,
    } = lowered;
    // #7280: re-read every shadow slot below the collection points that can run
    // under it. Whole-function, so it runs here rather than inside a lowering —
    // the shape it fixes is spread over dozens of lowerings, fifteen arms of
    // `index_set.rs` alone. It runs BEFORE any rendering path so the text
    // renderer and the in-process constructor see the same IR; a pass living in
    // one of them would silently not apply to the other.
    // See `crate::root_reload`.
    numeric_remainder::define_checked_number_remainder_if_used(&mut llmod);
    progress.phase(1, "lowering complete; finalizing generated IR");
    crate::root_reload::apply_to_module(&mut llmod);

    crate::typed_feedback_profile::finish_module(&mut llmod.native_rep_records);
    crate::verify_facts::finish(&mut llmod);

    let verify_native_regions = opts.verify_native_regions
        || std::env::var("PERRY_VERIFY_NATIVE_REGIONS").ok().as_deref() == Some("1");
    if verify_native_regions {
        crate::native_value::verify_native_rep_records(&llmod.native_rep_records)?;
    }

    crate::native_value::write_native_rep_artifact_if_enabled(
        &module_name,
        &llmod.native_rep_records,
    )?;

    if let Some((guid, _)) = crate::fallback_diag::module_probe() {
        llmod.declare_function(
            "llvm.pseudoprobe",
            "void",
            &[
                crate::types::I64,
                crate::types::I64,
                crate::types::I32,
                crate::types::I64,
            ],
        );
        llmod.add_raw_global(format!("!llvm.pseudo_probe_desc = !{{!3999999999}}\n!3999999999 = !{{i64 {guid}, i64 1, !\"perry-fallback\"}}"));
    }

    // #5391 codegen units: large modules split their object compilation into N
    // independently-compiled units so clang's peak RSS stays ~whole/N instead of
    // OOMing on one giant TU. Gated to large modules (default 1 unit = unchanged
    // behavior). `emit_ir_only` wants the whole-module text, so it takes the
    // single-text path; the split path avoids materializing the full ~1GB IR
    // string at all (which would defeat the memory win).
    // wasm32 WASI (#11378) takes the single-text path: its runtime-ABI pass
    // rewrites the whole module text before object emission. Splitting it
    // needs more than per-unit adaptation: the unit objects must be combined
    // with `wasm-ld -r` (the host `ld -r` cannot read wasm), each trimmed of
    // the buffer's trailing NUL, and the closure-ABI rewrite needs the
    // module-wide set of closure bodies. Until then a large WASI module is
    // compiled as one unit.
    // Every global exists now: fix the module's TLS model from this compile's
    // own output kind before any emission path renders it.
    if crate::expr::agent_ptr::program_tls_is_local_exec(&triple, &opts.output_type) {
        llmod.use_local_exec_tls();
        // The same fact: the runtime archive is linked into this image, so
        // its NaN-box operand table binds here (`inprocess/nanbox_operands.rs`).
        if triple.starts_with("x86_64") {
            llmod.read_nanbox_operands_from_runtime();
        }
    }
    let wasm32 = crate::target_layout::wasm32_lowering(&triple);
    let n_units = if opts.emit_ir_only || wasm32 {
        1
    } else {
        decide_codegen_units(callable_count, llmod.estimated_function_ir_bytes())
    };
    if n_units > 1 {
        if opts.output_type == "executable" {
            llmod.bind_generated_globals_to_image();
        }
        progress.phase(2, &format!("partitioning into {n_units} codegen units"));
        if let Some(result) =
            try_native_units(&mut llmod, n_units, opts.target.as_deref(), &module_prefix)
        {
            // `result` already contains the final object/archive here. The
            // generated `LlModule` can own millions of small allocations in a
            // minified bundle, and Rust must destroy that graph before this
            // function (and its `CompileProgress` guard) can return. On the
            // 8.8 MiB OpenCode code-mode chunk this took about five minutes
            // after LLVM reported 77/77, previously making the build look
            // stuck in LLVM. Name the real phase while the heartbeat thread is
            // still alive; a future arena-backed IR can make this O(arenas).
            progress.phase(3, "object ready; releasing generated IR");
            return result;
        }
        {
            let units = llmod.into_codegen_unit_texts(n_units);
            log::debug!(
                "perry-codegen: split '{}' into {} codegen units",
                module_name,
                units.len()
            );
            let save_dir = std::env::var("PERRY_SAVE_LL").ok();
            let units = units.enumerate().map(|(i, unit)| {
                if let Some(dir) = &save_dir {
                    let filename = format!("{dir}/{module_prefix}.unit{i}.ll");
                    let _ = std::fs::write(filename, &unit);
                }
                unit
            });
            match crate::linker::compile_streamed_units_to_object(units, opts.target.as_deref()) {
                Ok(object) => return Ok(object),
                Err(error) => return Err(error),
            }
        }
    }

    // exp/llvm-inprocess Phase 2: `PERRY_LLVM_INPROCESS=native` constructs
    // function bodies through the LLVM C API (only the module skeleton is
    // textual); `=diff` builds both arms and diffs them. Unit-split and
    // emit_ir_only paths above stay textual (they fall into the in-process
    // *transport* under these values, so no clang subprocess either way).
    if !wasm32 {
        if let Some(result) =
            try_native_construction(&mut llmod, opts.target.as_deref(), &module_prefix)
        {
            return result;
        }
    }

    {
        let ll_text = llmod.to_ir();
        drop(llmod);
        #[cfg(feature = "target-wasi")]
        let ll_text = if wasm32 {
            crate::wasm32::lower_module(&ll_text)
        } else {
            ll_text
        };
        log::debug!(
            "perry-codegen: emitted {} bytes of LLVM IR for '{}' ({} interned strings)",
            ll_text.len(),
            module_name,
            interned_strings
        );
        // Save .ll files when PERRY_SAVE_LL=<dir> is set
        if let Ok(save_dir) = std::env::var("PERRY_SAVE_LL") {
            let filename = format!("{}/{}.ll", save_dir, module_prefix);
            let _ = std::fs::write(&filename, &ll_text);
        }
        if opts.emit_ir_only {
            return Ok(ll_text.into_bytes());
        }
        match crate::linker::compile_owned_ll_to_object(ll_text, opts.target.as_deref()) {
            #[cfg(feature = "target-wasi")]
            Ok(object) if wasm32 => return Ok(crate::wasm32::trim_object(object)),
            Ok(object) => return Ok(object),
            Err(error) => return Err(error),
        }
    }
}

/// exp/llvm-inprocess: unit-split twin of [`try_native_construction`].
#[cfg(feature = "llvm-inprocess")]
fn try_native_units(
    llmod: &mut crate::module::LlModule,
    n_units: usize,
    target: Option<&str>,
    module_prefix: &str,
) -> Option<Result<Vec<u8>>> {
    // Personality-carrying Windows functions are parsed as small textual
    // islands inside each otherwise-native unit; ordinary functions still use
    // typed C-API construction. See `native_emit::freeze_unit`.
    match crate::native_emit::native_units_mode() {
        crate::native_emit::NativeMode::Off => None,
        crate::native_emit::NativeMode::Native => Some(
            crate::native_emit::compile_module_units_native(llmod, n_units, target, module_prefix),
        ),
        crate::native_emit::NativeMode::Diff => Some(
            crate::native_emit::compile_module_units_diff(llmod, n_units, target, module_prefix),
        ),
    }
}

#[cfg(not(feature = "llvm-inprocess"))]
fn try_native_units(
    _llmod: &mut crate::module::LlModule,
    _n_units: usize,
    _target: Option<&str>,
    _module_prefix: &str,
) -> Option<Result<Vec<u8>>> {
    None
}

/// exp/llvm-inprocess Phase 2 dispatch. `None` = native construction not
/// requested (or not compiled in) — continue on the text path. The
/// feature-off twin returns `None` unconditionally; a build without the
/// feature still fails loudly downstream in `compile_ll_to_object` when any
/// in-process mode is requested, so the flag can never silently no-op.
#[cfg(feature = "llvm-inprocess")]
fn try_native_construction(
    llmod: &mut crate::module::LlModule,
    target: Option<&str>,
    module_prefix: &str,
) -> Option<Result<Vec<u8>>> {
    // No target emits funclet EH any more (#7354 put windows-msvc on the same
    // invoke/landingpad shape as everywhere else), and the in-process reader
    // builds landing pads fine — so there is nothing left to decline here.
    match crate::native_emit::native_mode() {
        crate::native_emit::NativeMode::Off => None,
        crate::native_emit::NativeMode::Native => {
            let triple = llmod.target_triple.clone();
            let owned = std::mem::replace(llmod, LlModule::new(triple));
            Some(crate::native_emit::compile_module_native_owned(
                owned,
                target,
                module_prefix,
            ))
        }
        crate::native_emit::NativeMode::Diff => Some(crate::native_emit::compile_module_diff(
            llmod,
            target,
            module_prefix,
        )),
    }
}

#[cfg(not(feature = "llvm-inprocess"))]
fn try_native_construction(
    _llmod: &mut crate::module::LlModule,
    _target: Option<&str>,
    _module_prefix: &str,
) -> Option<Result<Vec<u8>>> {
    None
}
