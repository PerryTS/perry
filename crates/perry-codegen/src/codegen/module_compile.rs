//! Module compilation entry points and birth-shape prepass.
use super::{
    emit_lowered_module, ensure_layout_plan, lower_module, CompileOptions, HirModule, ModuleBirth,
};
use anyhow::Result;

/// Compile a Perry HIR module to an object file via LLVM IR.
///
/// CRITICAL (#686): `hir` MUST be `&HirModule` (shared reference), never
/// `&mut`. The caller computes `perry_hir::stable_hash::hash_module(hir)`
/// just before this call to derive the per-module object cache key. If
/// codegen ever mutated the HIR mid-compile, the cached `.o` would no
/// longer correspond to the hashed input and stale entries would be
/// served on subsequent builds. The `&` here is the load-bearing
/// guarantee — do not change to `&mut` without also moving the cache
/// hash to AFTER codegen.
pub fn compile_module(hir: &HirModule, mut opts: CompileOptions) -> Result<Vec<u8>> {
    ensure_layout_plan(hir, &mut opts)?;
    crate::facts_consumed::begin_module(&hir.name);
    crate::verify_facts::begin_module(
        &hir.name,
        opts.module_source.as_deref(),
        opts.debug_source_line_offset,
    );
    crate::verify_facts::bodies(&opts.program_facts);
    let _fallback = crate::fallback_diag::begin(
        &hir.name,
        opts.module_source.as_deref(),
        opts.debug_source_line_offset,
        opts.program_facts.fallback.clone(),
    );
    match lower_module(hir, opts, None)? {
        Some(lowered) => emit_lowered_module(lowered),
        None => unreachable!("normal lowering produces a module"),
    }
}

/// Compile an owned HIR input, releasing it at the lowering/LLVM boundary.
/// Lowering only borrows the input, preserving its object-cache identity.
pub fn compile_module_owned(hir: HirModule, mut opts: CompileOptions) -> Result<Vec<u8>> {
    ensure_layout_plan(&hir, &mut opts)?;
    crate::facts_consumed::begin_module(&hir.name);
    crate::verify_facts::begin_module(
        &hir.name,
        opts.module_source.as_deref(),
        opts.debug_source_line_offset,
    );
    crate::verify_facts::bodies(&opts.program_facts);
    let _fallback = crate::fallback_diag::begin(
        &hir.name,
        opts.module_source.as_deref(),
        opts.debug_source_line_offset,
        opts.program_facts.fallback.clone(),
    );
    let lowered = lower_module(&hir, opts, None)?;
    drop(hir);
    emit_lowered_module(lowered.expect("normal lowering produces a module"))
}

/// Design step 4's pre-pass: the content of every class birth this module's
/// string pool mints (local classes and imported stubs), from the SAME code
/// codegen runs — `compile_module` up to the header-image table, then an early
/// return. `opts` must be the options the module's codegen will get.
pub fn module_birth_shapes(hir: &HirModule, opts: CompileOptions) -> Result<Vec<ModuleBirth>> {
    if crate::verify_facts::enabled() {
        crate::verify_facts::begin_module(
            &hir.name,
            opts.module_source.as_deref(),
            opts.debug_source_line_offset,
        );
        crate::verify_facts::bodies(&opts.program_facts);
    }
    let mut births = Vec::new();
    lower_module(hir, opts, Some(&mut births))?;
    Ok(births)
}

/// Collect Exec facts from the very call instructions lowering emits. This
/// runs before the program's entry image is selected; no object or parallel
/// call registry is retained. In particular, optimized field reads contribute
/// their selected helper rather than a guess from the HIR expression kind.
pub fn module_realm_exec(
    hir: &HirModule,
    mut opts: CompileOptions,
    index: &perry_hir::realm_reach_index::RealmIndex,
) -> Result<std::collections::BTreeSet<String>> {
    ensure_layout_plan(hir, &mut opts)?;
    let lowered = lower_module(hir, opts, None)?.expect("normal lowering produces a module");
    Ok(lowered.llmod.realm_exec(index))
}
