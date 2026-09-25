//! Per-function fast-emit containment.
//!
//! A `TargetMachine` selects its machine pipeline for a whole module, so the
//! bounded machine pipeline chosen for one extreme function (see
//! [`super::DEFAULT_FAST_EMIT_MAX_INSTRS_X86_64`]) used to reach every
//! ordinary function in its codegen unit too. On the Claude Code bundle one
//! 988k-instruction factory closure demoted 950 siblings.
//!
//! This module moves the over-budget functions into a module of their own
//! after the IR pipeline has run, so each half is emitted by its own target
//! machine: the siblings by the unit's optimized one, the extreme functions
//! by the bounded one. The two emissions become two objects that the caller
//! partially links (`linker::merge_unit_objects`, the step that already joins
//! codegen units), so the rest of the backend never sees two objects.
//!
//! What has to hold across the cut:
//!
//! * Every internal symbol that the moved functions reference (functions,
//!   string constants, module globals) is defined on the sibling side and
//!   declared on the contained side. It is promoted to external linkage with
//!   hidden visibility under a name made unique by a hash of the unit's
//!   function set, so no two units — and no two Perry modules — can define
//!   the same promoted name. Hidden keeps it out of the final image's
//!   dynamic symbol table and keeps its references PC-relative.
//! * A moved function that was internal is promoted the same way, because its
//!   callers stay on the sibling side.
//! * Each object carries its own statepoint stack map for exactly the
//!   functions it defines, and the partial link concatenates the compact
//!   `.perry_gcmap` sections the way it already does for codegen units.
//! * Module-level `asm` (the Mach-O `.no_dead_strip` for the stack map) is
//!   kept on both sides; `llvm.used`-style appending arrays and every global
//!   initializer stay on the sibling side only.
//!
//! Shapes that this cut cannot express safely — an alias or ifunc anywhere in
//! the unit, a moved function in a comdat — decline containment, and the unit
//! falls back to whole-unit bounded emission exactly as before.

use std::collections::HashSet;

use inkwell::module::Module;
use llvm_sys::comdat::{LLVMGetComdat, LLVMSetComdat};
use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::{LLVMLinkage, LLVMTypeKind, LLVMVisibility};

/// Suffix infix of every promoted name, so a symbol table shows where it came
/// from.
pub(super) const PROMOTED_INFIX: &str = ".perry.fe.";

/// Why a unit could not be split. The caller logs it and keeps the old
/// whole-unit behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SplitDeclined(pub String);

impl std::fmt::Display for SplitDeclined {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether this host can partially link objects for `effective_target`.
///
/// The two halves are joined by `ld -r` (`linker::merge_unit_objects`). COFF
/// has no relocatable link (codegen units are archived there instead, and an
/// archive cannot nest inside another unit's archive), and a host linker
/// only reads its own object format — and, for GNU ld, its own architecture.
/// Everywhere else the unit keeps whole-unit bounded emission.
pub(super) fn split_emission_supported(effective_target: &str) -> bool {
    let arch = effective_target
        .split('-')
        .next()
        .unwrap_or(effective_target);
    let apple = effective_target.contains("apple");
    if effective_target.contains("windows") {
        return false;
    }
    if cfg!(target_os = "macos") {
        return apple;
    }
    if cfg!(target_os = "linux") {
        let host = std::env::consts::ARCH;
        let same_arch = match arch {
            "x86_64" | "x86_64h" | "amd64" => host == "x86_64",
            "aarch64" | "arm64" => host == "aarch64",
            _ => false,
        };
        return !apple && same_arch;
    }
    false
}

fn value_name(value: LLVMValueRef) -> String {
    let mut len = 0usize;
    let ptr = unsafe { LLVMGetValueName2(value, &mut len) };
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn set_value_name(value: LLVMValueRef, name: &str) {
    unsafe { LLVMSetValueName2(value, name.as_ptr() as *const _, name.len()) };
}

fn is_local(linkage: LLVMLinkage) -> bool {
    matches!(
        linkage,
        LLVMLinkage::LLVMInternalLinkage | LLVMLinkage::LLVMPrivateLinkage
    )
}

fn is_definition(global: LLVMValueRef) -> bool {
    unsafe { LLVMIsDeclaration(global) == 0 }
}

fn functions(module: LLVMModuleRef) -> Vec<LLVMValueRef> {
    let mut out = Vec::new();
    let mut f = unsafe { LLVMGetFirstFunction(module) };
    while !f.is_null() {
        out.push(f);
        f = unsafe { LLVMGetNextFunction(f) };
    }
    out
}

fn global_variables(module: LLVMModuleRef) -> Vec<LLVMValueRef> {
    let mut out = Vec::new();
    let mut g = unsafe { LLVMGetFirstGlobal(module) };
    while !g.is_null() {
        out.push(g);
        g = unsafe { LLVMGetNextGlobal(g) };
    }
    out
}

/// FNV-1a over every defined function name (module order) and the moved
/// set. Deterministic, so the object cache and reproducible builds see the
/// same promoted names every time; distinct per unit, because no two units
/// define the same set of functions.
fn unit_token(module: LLVMModuleRef, moved: &[LLVMValueRef]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes.iter().chain(std::iter::once(&0u8)) {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for f in functions(module) {
        if is_definition(f) {
            eat(value_name(f).as_bytes());
        }
    }
    eat(b"--moved--");
    for f in moved {
        eat(value_name(*f).as_bytes());
    }
    hash
}

/// Every internal/private global value a moved function body references,
/// through instruction operands and nested constant expressions (not through
/// other globals' initializers, which stay on the sibling side).
fn locals_referenced_by(function: LLVMValueRef) -> Vec<LLVMValueRef> {
    let mut seen_constants: HashSet<LLVMValueRef> = HashSet::new();
    let mut found: Vec<LLVMValueRef> = Vec::new();
    let mut found_set: HashSet<LLVMValueRef> = HashSet::new();
    let mut stack: Vec<LLVMValueRef> = Vec::new();

    let mut visit_operand = |value: LLVMValueRef, stack: &mut Vec<LLVMValueRef>| {
        if value.is_null() {
            return;
        }
        unsafe {
            if !LLVMIsAGlobalValue(value).is_null() {
                if is_local(LLVMGetLinkage(value)) && found_set.insert(value) {
                    found.push(value);
                }
            } else if !LLVMIsAConstant(value).is_null() && seen_constants.insert(value) {
                stack.push(value);
            }
        }
    };

    unsafe {
        if LLVMHasPersonalityFn(function) != 0 {
            visit_operand(LLVMGetPersonalityFn(function), &mut stack);
        }
        let mut bb = LLVMGetFirstBasicBlock(function);
        while !bb.is_null() {
            let mut inst = LLVMGetFirstInstruction(bb);
            while !inst.is_null() {
                let n = LLVMGetNumOperands(inst);
                for i in 0..n.max(0) as u32 {
                    visit_operand(LLVMGetOperand(inst, i), &mut stack);
                }
                inst = LLVMGetNextInstruction(inst);
            }
            bb = LLVMGetNextBasicBlock(bb);
        }
        while let Some(constant) = stack.pop() {
            let n = LLVMGetNumOperands(constant);
            for i in 0..n.max(0) as u32 {
                visit_operand(LLVMGetOperand(constant, i), &mut stack);
            }
        }
    }
    found
}

/// Give a local global value external linkage, hidden visibility and a
/// unit-unique name.
fn promote(global: LLVMValueRef, token: u64, anon: &mut usize) {
    let base = value_name(global);
    let base = if base.is_empty() {
        *anon += 1;
        format!("anon{}", *anon)
    } else {
        base
    };
    set_value_name(global, &format!("{base}{PROMOTED_INFIX}{token:016x}"));
    unsafe {
        LLVMSetLinkage(global, LLVMLinkage::LLVMExternalLinkage);
        LLVMSetVisibility(global, LLVMVisibility::LLVMHiddenVisibility);
    }
}

/// Remove a function's body in place, keeping its type,
/// attributes, calling convention, GC strategy and visibility — the things a
/// caller's code generation reads from the callee.
///
/// The C API has no `deleteBody`, so this is what `DeleteDeadBlocks` does:
/// replace every instruction's uses with poison, erase the instructions
/// (dropping their references to blocks and values), then delete the empty
/// blocks. The caller settles the linkage: a declaration may not be local.
pub(super) fn strip_body(function: LLVMValueRef) {
    unsafe {
        let mut bb = LLVMGetFirstBasicBlock(function);
        while !bb.is_null() {
            let mut inst = LLVMGetFirstInstruction(bb);
            while !inst.is_null() {
                let ty = LLVMTypeOf(inst);
                if LLVMGetTypeKind(ty) != LLVMTypeKind::LLVMVoidTypeKind
                    && !LLVMGetFirstUse(inst).is_null()
                {
                    LLVMReplaceAllUsesWith(inst, LLVMGetPoison(ty));
                }
                inst = LLVMGetNextInstruction(inst);
            }
            bb = LLVMGetNextBasicBlock(bb);
        }
        let mut bb = LLVMGetFirstBasicBlock(function);
        while !bb.is_null() {
            let mut inst = LLVMGetLastInstruction(bb);
            while !inst.is_null() {
                let prev = LLVMGetPreviousInstruction(inst);
                LLVMInstructionEraseFromParent(inst);
                inst = prev;
            }
            bb = LLVMGetNextBasicBlock(bb);
        }
        loop {
            let bb = LLVMGetFirstBasicBlock(function);
            if bb.is_null() {
                break;
            }
            LLVMDeleteBasicBlock(bb);
        }
        if LLVMHasPersonalityFn(function) != 0 {
            LLVMSetPersonalityFn(function, std::ptr::null_mut());
        }
        LLVMSetComdat(function, std::ptr::null_mut());
    }
}

/// A declaration may only be external (or extern_weak). Visibility is kept:
/// a hidden declaration is what lets the backend address its definition in
/// the other half PC-relatively.
fn declaration_linkage(global: LLVMValueRef) {
    unsafe {
        if LLVMGetLinkage(global) != LLVMLinkage::LLVMExternalWeakLinkage {
            LLVMSetLinkage(global, LLVMLinkage::LLVMExternalLinkage);
        }
    }
}

/// Split `module` in place: after this returns `Ok(Some(contained))`,
/// `module` defines every function except `moved`, and `contained` defines
/// exactly `moved` and declares everything they reference.
///
/// `Ok(None)`: every defined function in the unit is in `moved`, so there is
/// nothing to protect and the unit is emitted whole by the bounded machine.
///
/// `Err` means nothing was cut and `module` is still one emittable unit (the
/// only mutation that may already have happened is the promotion of locals,
/// which is correct for a single object too).
pub(super) fn split_moved_functions<'ctx>(
    module: &Module<'ctx>,
    moved_names: &[String],
) -> Result<Option<Module<'ctx>>, SplitDeclined> {
    #[cfg(test)]
    if TEST_DECLINE_SPLIT.with(std::cell::Cell::get) {
        return Err(SplitDeclined("declined by the test seam".into()));
    }
    let raw = module.as_mut_ptr();
    unsafe {
        if !LLVMGetFirstGlobalAlias(raw).is_null() {
            return Err(SplitDeclined("the unit defines a global alias".into()));
        }
        if !LLVMGetFirstGlobalIFunc(raw).is_null() {
            return Err(SplitDeclined("the unit defines an ifunc".into()));
        }
    }
    let mut moved: Vec<LLVMValueRef> = Vec::with_capacity(moved_names.len());
    for name in moved_names {
        let f = module
            .get_function(name)
            .ok_or_else(|| SplitDeclined(format!("`{name}` is not in the unit")))?;
        let f = inkwell::values::AsValueRef::as_value_ref(&f);
        if !is_definition(f) {
            return Err(SplitDeclined(format!("`{name}` has no body")));
        }
        if unsafe { !LLVMGetComdat(f).is_null() } {
            return Err(SplitDeclined(format!("`{name}` is in a comdat")));
        }
        moved.push(f);
    }
    let moved_set: HashSet<LLVMValueRef> = moved.iter().copied().collect();
    let siblings = functions(raw)
        .into_iter()
        .filter(|f| is_definition(*f) && !moved_set.contains(f))
        .count();
    if siblings == 0 {
        return Ok(None);
    }

    // 1. Promote, in the original, every local the cut would sever.
    let token = unit_token(raw, &moved);
    let mut to_promote: Vec<LLVMValueRef> = Vec::new();
    let mut promote_set: HashSet<LLVMValueRef> = HashSet::new();
    for f in &moved {
        if is_local(unsafe { LLVMGetLinkage(*f) }) && promote_set.insert(*f) {
            to_promote.push(*f);
        }
        for local in locals_referenced_by(*f) {
            if promote_set.insert(local) {
                to_promote.push(local);
            }
        }
    }
    let mut anon = 0usize;
    for global in &to_promote {
        promote(*global, token, &mut anon);
    }
    let moved_names_promoted: Vec<String> = moved.iter().map(|f| value_name(*f)).collect();

    // 2. Clone; the clone becomes the contained module.
    let contained = unsafe { Module::new(LLVMCloneModule(raw)) };
    let craw = contained.as_mut_ptr();
    let keep: HashSet<String> = moved_names_promoted.iter().cloned().collect();
    unsafe {
        for g in global_variables(craw) {
            if LLVMGetLinkage(g) == LLVMLinkage::LLVMAppendingLinkage {
                LLVMDeleteGlobal(g);
            }
        }
        // Locals keep their (now invalid for a declaration) linkage for one
        // more step: they are deleted below, never turned into external
        // declarations of a symbol nobody exports.
        for f in functions(craw) {
            if is_definition(f) && !keep.contains(&value_name(f)) {
                strip_body(f);
                if !is_local(LLVMGetLinkage(f)) {
                    declaration_linkage(f);
                }
            }
        }
        for g in global_variables(craw) {
            if is_definition(g) {
                LLVMSetInitializer(g, std::ptr::null_mut());
                LLVMSetComdat(g, std::ptr::null_mut());
                if !is_local(LLVMGetLinkage(g)) {
                    declaration_linkage(g);
                }
            }
        }
        // What is left local was referenced only by the stripped bodies and
        // initializers, so nothing uses it any more.
        for f in functions(craw) {
            if is_local(LLVMGetLinkage(f)) {
                if !LLVMGetFirstUse(f).is_null() {
                    return Err(SplitDeclined(format!(
                        "local function `{}` is still referenced after the cut",
                        value_name(f)
                    )));
                }
                LLVMDeleteFunction(f);
            }
        }
        for g in global_variables(craw) {
            if is_local(LLVMGetLinkage(g)) {
                if !LLVMGetFirstUse(g).is_null() {
                    return Err(SplitDeclined(format!(
                        "local global `{}` is still referenced after the cut",
                        value_name(g)
                    )));
                }
                LLVMDeleteGlobal(g);
            }
        }
    }
    // Drop every declaration nothing references any more (the unit's other
    // functions and globals): they emit nothing, but they are most of the
    // clone's symbol table.
    unsafe {
        for f in functions(craw) {
            if !is_definition(f) && LLVMGetFirstUse(f).is_null() {
                LLVMDeleteFunction(f);
            }
        }
        for g in global_variables(craw) {
            if !is_definition(g) && LLVMGetFirstUse(g).is_null() {
                LLVMDeleteGlobal(g);
            }
        }
    }
    contained.verify().map_err(|e| {
        SplitDeclined(format!(
            "the contained module does not verify:\n{}",
            e.to_string()
        ))
    })?;

    // 3. Only now cut the original: past this point there is no way back to
    // one module, and a verifier failure is a Perry bug.
    for f in &moved {
        strip_body(*f);
        declaration_linkage(*f);
    }
    Ok(Some(contained))
}

#[cfg(test)]
thread_local! {
    static TEST_DECLINE_SPLIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test seam: run `body` with containment declined, i.e. the pre-containment
/// whole-unit behaviour — the arm that proves the containment tests can fail.
#[cfg(test)]
pub(super) fn with_split_declined<T>(body: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_DECLINE_SPLIT.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(TEST_DECLINE_SPLIT.replace(true));
    body()
}

#[cfg(test)]
#[path = "fast_emit_split_tests.rs"]
mod tests;
