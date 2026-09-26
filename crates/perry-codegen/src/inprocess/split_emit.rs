//! #10586: emit the fast-emit budget's offenders in an object of their own.
//!
//! LLVM chooses the machine pipeline per `TargetMachine`, and a module is
//! emitted through exactly one of them, so before this split a single
//! over-budget function demoted every sibling in its unit to the O0 machine
//! pipeline too (measured on `@babel/parser`'s unit 0: 2.06 MiB of sibling
//! code, 168 of 282 functions changed — see
//! [`super::DEFAULT_FAST_EMIT_MAX_INSTRS_X86_64`]). That collateral is what
//! made the budget expensive enough that its value had to be argued target by
//! target, and it is target-independent: the aarch64 ceiling stays where two
//! measured 10 GiB blowups put it, while the ordinary code sharing a unit with
//! an extreme function no longer pays for it.
//!
//! The split runs on the module the IR pipeline has already optimized, so
//! neither half is re-optimized and nothing about inlining or IR shape
//! changes. The offenders' IR is exactly what the whole-unit fallback emitted;
//! the siblings' IR is exactly what they had before, and it now reaches the
//! optimized machine pipeline, which emits it as a unit without the extreme
//! function would have (`the_budget_no_longer_makes_ordinary_siblings_pay`).
//!
//! Mechanics, in order:
//!
//! 1. Every local (internal/private) symbol the two halves share — an offender
//!    itself, or any global or function an offender's body references — is
//!    renamed with a suffix derived from the unit's defined-function names and
//!    promoted to external linkage with hidden visibility, so the other half
//!    can reach it by name. Hidden keeps it out of any dynamic symbol table,
//!    and the suffix keeps two units' promoted `@.str.1`s from colliding in
//!    the final link.
//! 2. The module is cloned. The original keeps everything except the
//!    offenders' bodies; the clone keeps nothing except the offenders' bodies
//!    (every other function and every global variable becomes a declaration).
//! 3. The caller emits the original through the optimized target machine and
//!    the clone through the O0 one, and the two objects are combined exactly
//!    as codegen units already are (`linker::merge_unit_objects`).

use std::collections::HashSet;

use inkwell::module::Module;
use llvm_sys::comdat::LLVMSetComdat;
use llvm_sys::core::*;
use llvm_sys::prelude::{LLVMModuleRef, LLVMValueRef};
use llvm_sys::{LLVMLinkage, LLVMTypeKind, LLVMVisibility};

/// The two halves of a unit after [`split_offenders`]: `module` (the caller's
/// module, edited in place) keeps the siblings, and this owns the offenders.
pub(super) struct OffenderModule<'ctx> {
    pub module: Module<'ctx>,
}

/// Why a unit with offenders is still emitted whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholeUnitReason {
    /// Every defined function is over the budget: there is no sibling to free.
    NoSiblings,
    /// The module has a global alias or ifunc. Perry emits neither; one whose
    /// target moved to the other half would be invalid, so it is not
    /// attempted.
    AliasOrIfunc,
}

/// Can this unit's offenders be isolated from its siblings?
pub(super) fn whole_unit_reason(
    module: &Module<'_>,
    offenders: &HashSet<String>,
) -> Option<WholeUnitReason> {
    let m = module.as_mut_ptr();
    // SAFETY: read-only walks of a live module.
    unsafe {
        if !LLVMGetFirstGlobalAlias(m).is_null() || !LLVMGetFirstGlobalIFunc(m).is_null() {
            return Some(WholeUnitReason::AliasOrIfunc);
        }
        let mut f = LLVMGetFirstFunction(m);
        while !f.is_null() {
            if LLVMIsDeclaration(f) == 0 && !offenders.contains(&value_name(f)) {
                return None;
            }
            f = LLVMGetNextFunction(f);
        }
    }
    Some(WholeUnitReason::NoSiblings)
}

/// Split `module` in place: afterwards it defines every function except
/// `offenders`, and the returned module defines only `offenders`. Both are
/// verified before they are returned.
///
/// The caller must have checked [`whole_unit_reason`] first.
pub(super) fn split_offenders<'ctx>(
    module: &Module<'ctx>,
    offenders: &HashSet<String>,
) -> anyhow::Result<OffenderModule<'ctx>> {
    let m = module.as_mut_ptr();
    // SAFETY: every value handled below belongs to `m` (or, after the clone,
    // to the clone), both of which outlive this function; bodies are only
    // dropped through `strip_body`, which removes every intra-body use before
    // it erases anything.
    unsafe {
        let offender_fns = defined_functions(m)
            .into_iter()
            .filter(|&f| offenders.contains(&value_name(f)))
            .collect::<Vec<_>>();
        let suffix = promotion_suffix(m);

        // 1. Promote every local symbol the halves share.
        let mut shared: Vec<LLVMValueRef> = offender_fns.clone();
        let mut seen_globals: HashSet<LLVMValueRef> = HashSet::new();
        let mut seen_constants: HashSet<LLVMValueRef> = HashSet::new();
        for &f in &offender_fns {
            for_each_instruction(f, |inst| {
                for i in 0..LLVMGetNumOperands(inst).max(0) as u32 {
                    collect_global_refs(
                        LLVMGetOperand(inst, i),
                        &mut seen_globals,
                        &mut seen_constants,
                    );
                }
            });
            let personality = if LLVMHasPersonalityFn(f) != 0 {
                LLVMGetPersonalityFn(f)
            } else {
                std::ptr::null_mut()
            };
            if !personality.is_null() {
                collect_global_refs(personality, &mut seen_globals, &mut seen_constants);
            }
        }
        shared.extend(seen_globals);
        let mut anonymous = 0usize;
        for value in shared {
            promote_if_local(value, &suffix, &mut anonymous);
        }

        // 2. Clone after promotion so both halves agree on every name. The
        //    offenders are identified in the clone by their names AFTER
        //    promotion: a local offender was just renamed.
        let promoted_offenders: HashSet<String> =
            offender_fns.iter().map(|&f| value_name(f)).collect();
        let clone = Module::new(LLVMCloneModule(m));
        let c = clone.as_mut_ptr();

        for f in offender_fns {
            strip_body(f);
        }
        for f in defined_functions(c) {
            if !promoted_offenders.contains(&value_name(f)) {
                strip_body(f);
            }
        }
        declare_every_global(c);

        module.verify().map_err(|e| {
            anyhow::anyhow!(
                "fast-emit split left the sibling half invalid (this is a Perry codegen bug):\n{}",
                e.to_string()
            )
        })?;
        clone.verify().map_err(|e| {
            anyhow::anyhow!(
                "fast-emit split left the offender half invalid (this is a Perry codegen bug):\n{}",
                e.to_string()
            )
        })?;
        Ok(OffenderModule { module: clone })
    }
}

unsafe fn value_name(v: LLVMValueRef) -> String {
    let mut len = 0usize;
    let ptr = LLVMGetValueName2(v, &mut len);
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    String::from_utf8_lossy(std::slice::from_raw_parts(ptr as *const u8, len)).into_owned()
}

unsafe fn set_value_name(v: LLVMValueRef, name: &str) {
    LLVMSetValueName2(v, name.as_ptr() as *const _, name.len());
}

unsafe fn defined_functions(m: LLVMModuleRef) -> Vec<LLVMValueRef> {
    let mut out = Vec::new();
    let mut f = LLVMGetFirstFunction(m);
    while !f.is_null() {
        if LLVMIsDeclaration(f) == 0 {
            out.push(f);
        }
        f = LLVMGetNextFunction(f);
    }
    out
}

unsafe fn for_each_instruction(f: LLVMValueRef, mut visit: impl FnMut(LLVMValueRef)) {
    let mut bb = LLVMGetFirstBasicBlock(f);
    while !bb.is_null() {
        let mut inst = LLVMGetFirstInstruction(bb);
        while !inst.is_null() {
            visit(inst);
            inst = LLVMGetNextInstruction(inst);
        }
        bb = LLVMGetNextBasicBlock(bb);
    }
}

/// Every global value `v` reaches, looking through constant expressions and
/// aggregates (`getelementptr (@.str, …)`, a struct of function pointers).
/// Instructions, arguments, blocks and metadata are not globals and do not
/// lead to any.
unsafe fn collect_global_refs(
    v: LLVMValueRef,
    globals: &mut HashSet<LLVMValueRef>,
    constants: &mut HashSet<LLVMValueRef>,
) {
    if v.is_null() {
        return;
    }
    if !LLVMIsAGlobalValue(v).is_null() {
        globals.insert(v);
        return;
    }
    if LLVMIsAConstant(v).is_null() || !constants.insert(v) {
        return;
    }
    for i in 0..LLVMGetNumOperands(v).max(0) as u32 {
        collect_global_refs(LLVMGetOperand(v, i), globals, constants);
    }
}

fn is_local(linkage: LLVMLinkage) -> bool {
    matches!(
        linkage,
        LLVMLinkage::LLVMInternalLinkage | LLVMLinkage::LLVMPrivateLinkage
    )
}

unsafe fn promote_if_local(v: LLVMValueRef, suffix: &str, anonymous: &mut usize) {
    if !is_local(LLVMGetLinkage(v)) {
        return;
    }
    let name = value_name(v);
    let promoted = if name.is_empty() {
        *anonymous += 1;
        format!("__perry_fe_anon.{suffix}.{anonymous}")
    } else {
        format!("{name}.perry_fe.{suffix}")
    };
    set_value_name(v, &promoted);
    LLVMSetLinkage(v, LLVMLinkage::LLVMExternalLinkage);
    LLVMSetVisibility(v, LLVMVisibility::LLVMHiddenVisibility);
}

/// A name suffix unique to this unit: an FNV-1a hash of its defined
/// functions' names. External function names are unique program-wide, so two
/// units can only share a suffix if neither defines an external function —
/// and then a collision is a loud duplicate-symbol link error, never a silent
/// mis-binding. Deterministic, so the object cache and reproducible builds are
/// unaffected.
unsafe fn promotion_suffix(m: LLVMModuleRef) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for f in defined_functions(m) {
        for byte in value_name(f).bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

/// Turn a definition into a declaration, keeping the function value itself
/// (so its uses, attributes and calling convention stay as they are).
///
/// Every intra-body use is removed before anything is erased: each
/// instruction's result is replaced with a null constant of its type (the
/// only constant a `token` — `gc.statepoint`'s result — can be), so erasing
/// the instructions in any order never leaves a use behind, and the blocks
/// are erased last, once no branch names them.
unsafe fn strip_body(f: LLVMValueRef) {
    for_each_instruction(f, |inst| {
        let ty = LLVMTypeOf(inst);
        if LLVMGetTypeKind(ty) != LLVMTypeKind::LLVMVoidTypeKind {
            LLVMReplaceAllUsesWith(inst, LLVMConstNull(ty));
        }
    });
    let mut bb = LLVMGetFirstBasicBlock(f);
    while !bb.is_null() {
        let mut inst = LLVMGetFirstInstruction(bb);
        while !inst.is_null() {
            LLVMInstructionEraseFromParent(inst);
            inst = LLVMGetFirstInstruction(bb);
        }
        bb = LLVMGetNextBasicBlock(bb);
    }
    let mut bb = LLVMGetFirstBasicBlock(f);
    while !bb.is_null() {
        LLVMDeleteBasicBlock(bb);
        bb = LLVMGetFirstBasicBlock(f);
    }
    make_declaration_linkage(f);
    if LLVMHasPersonalityFn(f) != 0 {
        LLVMSetPersonalityFn(f, std::ptr::null_mut());
    }
    LLVMGlobalClearMetadata(f);
}

/// A declaration may not be local, may not sit in a comdat, and is only ever
/// external or extern_weak. A local reaching here is one nothing in this half
/// references (the shared ones were promoted), so its unreferenced declaration
/// emits no symbol at all.
unsafe fn make_declaration_linkage(v: LLVMValueRef) {
    LLVMSetComdat(v, std::ptr::null_mut());
    if LLVMGetLinkage(v) != LLVMLinkage::LLVMExternalWeakLinkage {
        LLVMSetLinkage(v, LLVMLinkage::LLVMExternalLinkage);
    }
}

/// In the offender half every global variable is defined by the sibling half,
/// so each becomes a declaration. Appending globals (`llvm.global_ctors` and
/// friends) would be registered twice and are removed instead; Perry emits
/// none, so this is defensive.
unsafe fn declare_every_global(m: LLVMModuleRef) {
    let mut appending = Vec::new();
    let mut g = LLVMGetFirstGlobal(m);
    while !g.is_null() {
        if LLVMGetLinkage(g) == LLVMLinkage::LLVMAppendingLinkage {
            appending.push(g);
        } else if LLVMIsDeclaration(g) == 0 {
            LLVMSetInitializer(g, std::ptr::null_mut());
            make_declaration_linkage(g);
            LLVMGlobalClearMetadata(g);
        }
        g = LLVMGetNextGlobal(g);
    }
    for g in appending {
        LLVMDeleteGlobal(g);
    }
}

#[cfg(all(test, unix))]
#[path = "split_emit_tests.rs"]
mod tests;
