//! Exact liveness of GC values across RS4GC safepoints (RFC "deferred
//! collection", step S4; #11528).
//!
//! `rewrite-statepoints-for-gc` (RS4GC) emits one `gc.relocate` per GC value
//! live across each safepoint, and two for a safepoint that is an `invoke`:
//! one on the normal edge and one in the landing pad. That relocation count is
//! what makes RS4GC and the optimizer after it slow on large functions (#8583).
//! This module computes it **before** RS4GC runs, on the module RS4GC is about
//! to rewrite, so the shadow-frame spill decision is a budget on the real
//! number instead of on `(slots + sites) × sites`, which overshot the
//! claude-code bundle by about 100× (1.34e9 estimated vs 13.9 M real).
//!
//! # Where it runs, and why there
//!
//! The production pipeline is `always-inline,function(mem2reg,sccp),
//! rewrite-statepoints-for-gc` ([`crate::linker::STATEPOINT_REWRITE_PASSES`]).
//! The backend runs its first half ([`crate::linker::STATEPOINT_PREPARE_PASSES`]),
//! calls [`analyze_function`] on every function that carries the statepoint GC
//! strategy, and only then runs RS4GC itself. The input is therefore exactly
//! RS4GC's input: after inlining, after `mem2reg` turned root allocas into SSA
//! values, and after `sccp` folded constants. Everything that decides whether a
//! call is a safepoint is already an attribute on the call by then — the
//! audited `gc-leaf-function` marking (S0, S1's generated call-effects table),
//! the IC fast paths (S2) — and a root that S3 rematerializes from its global
//! is a fresh load after each safepoint, not a value live across it. So this
//! analysis needs no knowledge of Perry's lowering, and cannot drift from it.
//! Shadow-stack targets never get here: their functions carry no GC strategy.
//!
//! Computing it on the textual IR before `mem2reg` (in `precise_roots.rs`) was
//! the alternative. It would have to re-derive `mem2reg` and `sccp` — which
//! slot stores fold to constants, which call results are dead — and would
//! miss the always-inlined helpers' calls. The LLVM-level count is exact at
//! the cost of splitting one `run_passes` call in two.
//!
//! # What it models
//!
//! RS4GC's liveness (`computeLiveInValues`/`findLiveSetAtInst`): a value of a
//! GC pointer type (`ptr addrspace(1)`, or a vector of them) that is not a
//! `Constant` is live across a safepoint when it is live *into* the call: a
//! use is reachable from the call without passing the value's definition. A
//! phi's operand is a use at the end of the incoming block. The call's own
//! result is not live across it, but its GC-pointer arguments are, even when
//! the call is their last use (RS4GC's `findLiveSetAtInst` walks the call's
//! operands too; Perry passes NaN-boxed `i64`s, so this is rare in its IR).
//! Before computing
//! liveness RS4GC also edits the function, and each edit changes the count:
//!
//! - `removeUnreachableBlocks` (`markAliveBlocks` in `Local.cpp`): code after
//!   a `noreturn` call, a store to null/undef, an `assume(false)` and a call
//!   through null/undef is deleted; an `invoke` of a `nounwind` callee becomes
//!   a `call` (one relocation edge, not two), or disappears when its result is
//!   unused and it has no side effects; an `invoke` of a `noreturn` callee
//!   loses its normal successor; constant branch conditions and identical
//!   branch targets are folded; unreachable blocks are deleted, and a phi that
//!   lost an incoming edge folds when its remaining inputs agree;
//! - `FoldSingleEntryPHINodes` on every block with a unique predecessor;
//! - moving a single-use `icmp` that feeds a conditional branch down to the
//!   branch, which extends its operands' live ranges to the terminator.
//!
//! A call is a safepoint unless it is inline asm, carries (or its callee
//! carries) `"gc-leaf-function"`, is an intrinsic other than
//! `gc.statepoint`'s deopt/element-atomic memcpy family, or calls a C library
//! function LLVM's `TargetLibraryInfo` knows ([`LIBFUNCS`]).
//!
//! Base pointers (`findBasePointer`): RS4GC relocates a value's *base* too.
//! Perry's phis and selects often merge a NaN-box tag constant
//! (`inttoptr (i64 0x7FFC… to ptr addrspace(1))`) with a heap pointer. The
//! constant's base is `null`, the pointer's is itself, so RS4GC clones the
//! phi into a `.base` phi that is live wherever the phi is: two relocations
//! per crossing. A phi of constants only has a constant base and is dropped
//! from the live set; a phi whose inputs are all bases (or `null`) is its own
//! base. The pruning and the optimistic meet are modeled as RS4GC runs them.
//!
//! One thing is bounded rather than exact: a `gep`, `addrspacecast`,
//! `bitcast` or `freeze` of a GC pointer (or a phi/select whose inputs agree
//! on one *other* existing base) makes RS4GC add that existing base to the
//! live set, where it may already be, or rematerialize the derived value.
//! Each adds at most one relocation per safepoint it crosses, so
//! [`FunctionLiveness::relocation_bound`] is an upper bound and
//! [`FunctionLiveness::is_exact`] says whether it is also exact. Perry's
//! lowering emits none of these today. The audit
//! (`PERRY_CODEGEN_UNIT_TIMINGS`) marks any function where the count is only
//! a bound, and every function where the prediction and RS4GC's real output
//! disagree.
//!
//! The count only chooses a *compile-time* lowering. RS4GC still decides, on
//! its own, what to relocate, so an error here can cost compile time but can
//! never make a root imprecise.
//!
//! # Cost
//!
//! One scan of the instructions, then a backward walk per GC value over the
//! blocks where it is live (the census's `gcm-stats` algorithm). That is
//! `O(instructions + Σ_v |live blocks of v|)`: linear in the IR plus the
//! liveness it reports, never more than RS4GC's own liveness, which
//! materializes the same (value, block) pairs as sets. Per-safepoint counts
//! come from a difference array, so a value live through a block with many
//! safepoints costs O(log n) there, not O(n).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::OnceLock;

use llvm_sys::core::*;
use llvm_sys::prelude::{LLVMBasicBlockRef, LLVMTypeRef, LLVMValueRef};
use llvm_sys::{LLVMOpcode, LLVMTailCallKind, LLVMTypeKind};

/// Live GC values across one safepoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SafepointLiveness {
    /// Index of the block in function order.
    pub block: u32,
    /// Index of the call within its block, in the analyzed (post-`sccp`) IR.
    pub index: u32,
    /// Relocation edges: 1 for a call, 2 for an `invoke` (normal + unwind).
    pub edges: u8,
    /// Values RS4GC relocates across the call, once per edge: the GC values
    /// live across it, plus the fresh base of each live phi/select that
    /// needs one, minus live values whose base is a constant.
    pub live: u32,
}

/// The liveness census of one function, as RS4GC will see it.
///
/// `safepoints` is the internal API for S4b (rooting slow-path call sites in
/// GC-map stack slots): it lists every call RS4GC will turn into a statepoint,
/// in block order, with the number of values live across it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FunctionLiveness {
    pub instructions: usize,
    pub blocks: usize,
    /// GC-pointer SSA values (instructions and arguments) after RS4GC's edits.
    pub gc_values: usize,
    /// `gep`/cast/`freeze` values of a GC pointer, whose base RS4GC must
    /// relocate alongside them (see the module docs).
    pub derived_values: usize,
    /// Phis/selects RS4GC gives a fresh base phi/select (a phi that merges a
    /// tagged constant with a heap pointer, say): each crossing costs two
    /// relocations, the value and its base.
    pub fresh_bases: usize,
    pub safepoints: Vec<SafepointLiveness>,
    /// `Σ live × edges` over the safepoints: RS4GC's `gc.relocate` count
    /// when [`Self::is_exact`].
    pub relocations: u64,
    /// `Σ edges` over (possibly-derived value, safepoint it crosses) pairs:
    /// the most base pointers RS4GC can add on top of `relocations`.
    pub derived_crossings: u64,
    /// Steps taken by the liveness walk (uses + live blocks + pred edges).
    pub work: u64,
    /// Wall time of the analysis, in microseconds (set by
    /// [`analyze_module`]; reported by the audit).
    pub micros: u64,
}

impl FunctionLiveness {
    pub fn statepoints(&self) -> usize {
        self.safepoints.len()
    }

    pub fn invoke_statepoints(&self) -> usize {
        self.safepoints.iter().filter(|s| s.edges == 2).count()
    }

    pub fn max_live(&self) -> u32 {
        self.safepoints.iter().map(|s| s.live).max().unwrap_or(0)
    }

    /// No derived pointer crosses a safepoint, so `relocations` is exact.
    pub fn is_exact(&self) -> bool {
        self.derived_crossings == 0
    }

    /// Upper bound on the relocations RS4GC emits; equal to `relocations`
    /// when [`Self::is_exact`]. The spill decision budgets this number.
    pub fn relocation_bound(&self) -> u64 {
        self.relocations.saturating_add(self.derived_crossings)
    }
}

/// Every function of `module` named in `functions` (the ones carrying the
/// statepoint GC strategy), with its liveness. Order follows the module.
pub(crate) fn analyze_module(
    module: &inkwell::module::Module<'_>,
    functions: &std::collections::HashSet<String>,
) -> Vec<(String, FunctionLiveness)> {
    let mut out = Vec::new();
    let mut function = module.get_first_function();
    while let Some(f) = function {
        if f.count_basic_blocks() > 0 {
            let name = f.get_name().to_string_lossy();
            if functions.contains(name.as_ref()) {
                use inkwell::values::AsValueRef;
                let started = std::time::Instant::now();
                let mut l = analyze_function(f.as_value_ref());
                l.micros = started.elapsed().as_micros() as u64;
                out.push((name.into_owned(), l));
            }
        }
        function = f.get_next_function();
    }
    out
}

/// Count `gc.relocate` calls in a rewritten function: the ground truth the
/// prediction is audited against.
pub(crate) fn count_relocates(function: LLVMValueRef) -> u64 {
    let ids = ids();
    let mut n = 0u64;
    unsafe {
        let mut bb = LLVMGetFirstBasicBlock(function);
        while !bb.is_null() {
            let mut i = LLVMGetFirstInstruction(bb);
            while !i.is_null() {
                if LLVMGetInstructionOpcode(i) == LLVMOpcode::LLVMCall {
                    let callee = LLVMIsAFunction(LLVMGetCalledValue(i));
                    if !callee.is_null() && LLVMGetIntrinsicID(callee) == ids.gc_relocate {
                        n += 1;
                    }
                }
                i = LLVMGetNextInstruction(i);
            }
            bb = LLVMGetNextBasicBlock(bb);
        }
    }
    n
}

/// Analyze one function. `function` must be a defined function of a live
/// module.
pub(crate) fn analyze_function(function: LLVMValueRef) -> FunctionLiveness {
    // SAFETY: the caller hands us a function of a module it owns and does not
    // mutate during the call; every C API below only reads the IR.
    unsafe { analyze(function) }
}

// ---------------------------------------------------------------------------
// Attribute kinds and intrinsic ids (process-global in LLVM).

struct Ids {
    noreturn: u32,
    nounwind: u32,
    willreturn: u32,
    memory: u32,
    assume: u32,
    deoptimize: u32,
    memcpy_atomic: u32,
    memmove_atomic: u32,
    gc_relocate: u32,
}

fn ids() -> &'static Ids {
    static IDS: OnceLock<Ids> = OnceLock::new();
    IDS.get_or_init(|| unsafe {
        let attr = |n: &str| LLVMGetEnumAttributeKindForName(n.as_ptr().cast(), n.len());
        let intr = |n: &str| LLVMLookupIntrinsicID(n.as_ptr().cast(), n.len());
        Ids {
            noreturn: attr("noreturn"),
            nounwind: attr("nounwind"),
            willreturn: attr("willreturn"),
            memory: attr("memory"),
            assume: intr("llvm.assume"),
            deoptimize: intr("llvm.experimental.deoptimize"),
            memcpy_atomic: intr("llvm.memcpy.element.unordered.atomic"),
            memmove_atomic: intr("llvm.memmove.element.unordered.atomic"),
            gc_relocate: intr("llvm.experimental.gc.relocate"),
        }
    })
}

// ---------------------------------------------------------------------------
// Small helpers over the C API.

/// Pointer-identity hashing for `LLVMValueRef`/`LLVMBasicBlockRef` keys.
#[derive(Default)]
struct PtrHasher(u64);

impl Hasher for PtrHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(b)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_usize(&mut self, v: usize) {
        self.0 = ((v as u64) >> 3).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type PtrMap<K, V> = HashMap<K, V, BuildHasherDefault<PtrHasher>>;

unsafe fn is_gc_type(ty: LLVMTypeRef) -> bool {
    match LLVMGetTypeKind(ty) {
        LLVMTypeKind::LLVMPointerTypeKind => LLVMGetPointerAddressSpace(ty) == 1,
        LLVMTypeKind::LLVMVectorTypeKind | LLVMTypeKind::LLVMScalableVectorTypeKind => {
            let e = LLVMGetElementType(ty);
            LLVMGetTypeKind(e) == LLVMTypeKind::LLVMPointerTypeKind
                && LLVMGetPointerAddressSpace(e) == 1
        }
        _ => false,
    }
}

unsafe fn called_function(call: LLVMValueRef) -> LLVMValueRef {
    LLVMIsAFunction(LLVMGetCalledValue(call))
}

/// `CallBase::hasFnAttr(kind)`: on the call site or on the called function.
unsafe fn has_fn_attr(call: LLVMValueRef, kind: u32) -> bool {
    if !LLVMGetCallSiteEnumAttribute(call, llvm_sys::LLVMAttributeFunctionIndex, kind).is_null() {
        return true;
    }
    let f = called_function(call);
    !f.is_null()
        && !LLVMGetEnumAttributeAtIndex(f, llvm_sys::LLVMAttributeFunctionIndex, kind).is_null()
}

unsafe fn has_fn_string_attr(call: LLVMValueRef, name: &str) -> bool {
    let idx = llvm_sys::LLVMAttributeFunctionIndex;
    let (p, n) = (name.as_ptr().cast(), name.len() as u32);
    if !LLVMGetCallSiteStringAttribute(call, idx, p, n).is_null() {
        return true;
    }
    let f = called_function(call);
    !f.is_null() && !LLVMGetStringAttributeAtIndex(f, idx, p, n).is_null()
}

unsafe fn value_name<'a>(v: LLVMValueRef) -> &'a [u8] {
    let mut len = 0usize;
    let p = LLVMGetValueName2(v, &mut len);
    if p.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(p.cast(), len)
    }
}

/// A null pointer in address space 0 (where LLVM treats null as undefined).
unsafe fn is_null_as0(v: LLVMValueRef) -> bool {
    let ty = LLVMTypeOf(v);
    LLVMGetTypeKind(ty) == LLVMTypeKind::LLVMPointerTypeKind
        && LLVMGetPointerAddressSpace(ty) == 0
        && LLVMIsConstant(v) != 0
        && LLVMIsNull(v) != 0
}

unsafe fn is_false_or_undef(v: LLVMValueRef) -> bool {
    LLVMIsUndef(v) != 0 || (!LLVMIsAConstantInt(v).is_null() && LLVMConstIntGetZExtValue(v) == 0)
}

/// `CallBase::getMemoryEffects().onlyReadsMemory()`: the `memory` attribute
/// packs two bits per location, `Ref` = 1 and `Mod` = 2.
unsafe fn only_reads_memory(call: LLVMValueRef) -> bool {
    let ids = ids();
    let unknown = u64::MAX;
    let read = |a: llvm_sys::prelude::LLVMAttributeRef| {
        if a.is_null() {
            unknown
        } else {
            LLVMGetEnumAttributeValue(a)
        }
    };
    let site = read(LLVMGetCallSiteEnumAttribute(
        call,
        llvm_sys::LLVMAttributeFunctionIndex,
        ids.memory,
    ));
    let f = called_function(call);
    let callee = if f.is_null() {
        unknown
    } else {
        read(LLVMGetEnumAttributeAtIndex(
            f,
            llvm_sys::LLVMAttributeFunctionIndex,
            ids.memory,
        ))
    };
    (site & callee) & 0xAAAA_AAAA_AAAA_AAAA == 0
}

/// Personalities that catch asynchronous exceptions keep `invoke`s of
/// `nounwind` callees (`canSimplifyInvokeNoUnwind`).
unsafe fn can_simplify_invoke_nounwind(function: LLVMValueRef) -> bool {
    if LLVMHasPersonalityFn(function) == 0 {
        return true;
    }
    let p = LLVMGetPersonalityFn(function);
    let name = value_name(p);
    !matches!(
        name,
        b"_except_handler3" | b"_except_handler4" | b"__C_specific_handler"
    )
}

/// A compare whose only use is the branch being folded: deleted with it.
unsafe fn trivially_dead_after_branch(cond: LLVMValueRef) -> bool {
    if LLVMIsAInstruction(cond).is_null()
        || !matches!(
            LLVMGetInstructionOpcode(cond),
            LLVMOpcode::LLVMICmp | LLVMOpcode::LLVMFCmp
        )
    {
        return false;
    }
    let first = LLVMGetFirstUse(cond);
    !first.is_null() && LLVMGetNextUse(first).is_null()
}

/// RS4GC's `NeedsRewrite` for a call site that survives its CFG cleanup.
unsafe fn needs_statepoint(call: LLVMValueRef) -> bool {
    let ids = ids();
    if !LLVMIsAInlineAsm(LLVMGetCalledValue(call)).is_null() {
        return false;
    }
    if has_fn_string_attr(call, "gc-leaf-function") {
        return false;
    }
    let f = called_function(call);
    if !f.is_null() {
        let iid = LLVMGetIntrinsicID(f);
        if iid != 0 {
            // Most intrinsics are leaves. `gc.statepoint` itself is skipped
            // by `NeedsRewrite`; these three are not leaves.
            return iid == ids.deoptimize || iid == ids.memcpy_atomic || iid == ids.memmove_atomic;
        }
        if is_libfunc(value_name(f)) {
            return false;
        }
    }
    true
}

fn is_libfunc(name: &[u8]) -> bool {
    LIBFUNCS
        .binary_search_by(|probe| probe.as_bytes().cmp(name))
        .is_ok()
}

// ---------------------------------------------------------------------------
// The analysis.

const LIVE_OUT: u32 = u32::MAX;

/// What RS4GC's cleanup makes of a block's terminating `invoke`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InvokeFate {
    /// Not an invoke (or removed as unreachable).
    None,
    /// Still a safepoint, relocated on this many edges.
    Edges(u8),
    /// A side-effect-free `nounwind` invoke whose result is unused: deleted.
    Erased,
}

unsafe fn analyze(function: LLVMValueRef) -> FunctionLiveness {
    let ids = ids();
    let mut out = FunctionLiveness::default();

    // Blocks and instructions, CSR by block.
    let mut blocks: Vec<LLVMBasicBlockRef> = Vec::new();
    let mut bb = LLVMGetFirstBasicBlock(function);
    while !bb.is_null() {
        blocks.push(bb);
        bb = LLVMGetNextBasicBlock(bb);
    }
    let nb = blocks.len();
    if nb == 0 {
        return out;
    }
    let bidx: PtrMap<LLVMBasicBlockRef, u32> = blocks
        .iter()
        .enumerate()
        .map(|(i, &b)| (b, i as u32))
        .collect();
    let mut insts: Vec<LLVMValueRef> = Vec::new();
    let mut start: Vec<usize> = Vec::with_capacity(nb + 1);
    for &b in &blocks {
        start.push(insts.len());
        let mut i = LLVMGetFirstInstruction(b);
        while !i.is_null() {
            insts.push(i);
            i = LLVMGetNextInstruction(i);
        }
    }
    start.push(insts.len());
    out.instructions = insts.len();
    out.blocks = nb;

    // ---- markAliveBlocks: surviving prefix of each block, and its
    // successors after constant folding (sorted, deduplicated).
    let simplify_nounwind = can_simplify_invoke_nounwind(function);
    let mut live_len = vec![0u32; nb];
    let mut fate = vec![InvokeFate::None; nb];
    let mut succ_start: Vec<usize> = Vec::with_capacity(nb + 1);
    let mut succs: Vec<u32> = Vec::new();
    // Conditions deleted with a branch whose two targets are equal.
    let mut deleted: PtrMap<LLVMValueRef, ()> = PtrMap::default();
    // Conditional branches that survive: (block, condition) — the `icmp`
    // move candidates.
    let mut cond_branches: Vec<(u32, LLVMValueRef)> = Vec::new();
    for b in 0..nb {
        succ_start.push(succs.len());
        let (s, e) = (start[b], start[b + 1]);
        let mut cut: Option<usize> = None;
        for k in s..e {
            let i = insts[k];
            match LLVMGetInstructionOpcode(i) {
                LLVMOpcode::LLVMCall => {
                    let callee = LLVMGetCalledValue(i);
                    let f = LLVMIsAFunction(callee);
                    if !f.is_null() {
                        if LLVMGetIntrinsicID(f) == ids.assume
                            && is_false_or_undef(LLVMGetOperand(i, 0))
                        {
                            cut = Some(k - s);
                            break;
                        }
                    } else if is_null_as0(callee) || LLVMIsUndef(callee) != 0 {
                        cut = Some(k - s);
                        break;
                    }
                    if has_fn_attr(i, ids.noreturn)
                        && LLVMGetTailCallKind(i) != LLVMTailCallKind::LLVMTailCallKindMustTail
                    {
                        cut = Some(k - s + 1);
                        break;
                    }
                }
                LLVMOpcode::LLVMStore => {
                    if LLVMGetVolatile(i) == 0 {
                        let p = LLVMGetOperand(i, 1);
                        if LLVMIsUndef(p) != 0 || is_null_as0(p) {
                            cut = Some(k - s);
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(c) = cut {
            live_len[b] = c as u32;
            continue;
        }
        live_len[b] = (e - s) as u32;
        if e == s {
            continue;
        }
        let term = insts[e - 1];
        let first = succs.len();
        let push = |succs: &mut Vec<u32>, target: LLVMBasicBlockRef| {
            succs.push(bidx[&target]);
        };
        match LLVMGetInstructionOpcode(term) {
            LLVMOpcode::LLVMInvoke => {
                let callee = LLVMGetCalledValue(term);
                if LLVMIsAFunction(callee).is_null()
                    && (is_null_as0(callee) || LLVMIsUndef(callee) != 0)
                {
                    live_len[b] -= 1;
                    continue;
                }
                let noreturn = has_fn_attr(term, ids.noreturn);
                let nounwind = simplify_nounwind && has_fn_attr(term, ids.nounwind);
                if nounwind
                    && LLVMGetFirstUse(term).is_null()
                    && has_fn_attr(term, ids.willreturn)
                    && only_reads_memory(term)
                {
                    fate[b] = InvokeFate::Erased;
                } else {
                    fate[b] = InvokeFate::Edges(if nounwind { 1 } else { 2 });
                }
                if !noreturn {
                    push(&mut succs, LLVMGetNormalDest(term));
                }
                if !nounwind {
                    push(&mut succs, LLVMGetUnwindDest(term));
                }
            }
            LLVMOpcode::LLVMBr => {
                if LLVMIsConditional(term) != 0 {
                    let cond = LLVMGetCondition(term);
                    let (t, f) = (LLVMGetSuccessor(term, 0), LLVMGetSuccessor(term, 1));
                    if !LLVMIsAConstantInt(cond).is_null() {
                        push(
                            &mut succs,
                            if LLVMConstIntGetZExtValue(cond) != 0 {
                                t
                            } else {
                                f
                            },
                        );
                    } else if t == f {
                        // ConstantFoldTerminator(DeleteDeadConditions): the
                        // condition goes too when the branch was its only use.
                        push(&mut succs, t);
                        if trivially_dead_after_branch(cond) {
                            deleted.insert(cond, ());
                        }
                    } else {
                        push(&mut succs, t);
                        push(&mut succs, f);
                        if !LLVMIsAInstruction(cond).is_null()
                            && LLVMGetInstructionOpcode(cond) == LLVMOpcode::LLVMICmp
                        {
                            cond_branches.push((b as u32, cond));
                        }
                    }
                } else {
                    push(&mut succs, LLVMGetSuccessor(term, 0));
                }
            }
            LLVMOpcode::LLVMSwitch => {
                let cond = LLVMGetOperand(term, 0);
                let n = LLVMGetNumSuccessors(term);
                if !LLVMIsAConstantInt(cond).is_null() {
                    let mut dest = LLVMGetSuccessor(term, 0);
                    for c in 1..n {
                        if LLVMGetOperand(term, 2 * c) == cond {
                            dest = LLVMGetSuccessor(term, c);
                            break;
                        }
                    }
                    push(&mut succs, dest);
                } else {
                    for c in 0..n {
                        push(&mut succs, LLVMGetSuccessor(term, c));
                    }
                }
            }
            _ => {
                for c in 0..LLVMGetNumSuccessors(term) {
                    push(&mut succs, LLVMGetSuccessor(term, c));
                }
            }
        }
        let tail = &mut succs[first..];
        tail.sort_unstable();
        let mut w = first;
        for r in first..succs.len() {
            if r == first || succs[r] != succs[w - 1] {
                succs[w] = succs[r];
                w += 1;
            }
        }
        succs.truncate(w);
    }
    succ_start.push(succs.len());
    let succ_of = |b: usize| &succs[succ_start[b]..succ_start[b + 1]];

    // Reachability from the entry over the surviving edges.
    let mut reach = vec![false; nb];
    let mut stack = vec![0usize];
    reach[0] = true;
    while let Some(b) = stack.pop() {
        for &s in succ_of(b) {
            if !reach[s as usize] {
                reach[s as usize] = true;
                stack.push(s as usize);
            }
        }
    }
    // Predecessors, restricted to reachable blocks and surviving edges.
    let mut pred_count = vec![0usize; nb + 1];
    for b in (0..nb).filter(|&b| reach[b]) {
        for &s in succ_of(b) {
            pred_count[s as usize + 1] += 1;
        }
    }
    for b in 0..nb {
        pred_count[b + 1] += pred_count[b];
    }
    let pred_start = pred_count;
    let mut preds = vec![0u32; pred_start[nb]];
    let mut fill = pred_start.clone();
    for b in (0..nb).filter(|&b| reach[b]) {
        for &s in succ_of(b) {
            preds[fill[s as usize]] = b as u32;
            fill[s as usize] += 1;
        }
    }
    let pred_of = |b: usize| &preds[pred_start[b]..pred_start[b + 1]];
    let has_edge = |p: usize, b: u32| succ_of(p).binary_search(&b).is_ok();

    // ---- phi folding: removePredecessor's `hasConstantValue` fold for phis
    // that lost an edge, then FoldSingleEntryPHINodes. A null alias target
    // stands for `poison` (a phi of only itself).
    let mut alias: PtrMap<LLVMValueRef, LLVMValueRef> = PtrMap::default();
    let mut vals: Vec<LLVMValueRef> = Vec::new();
    for b in (0..nb).filter(|&b| reach[b]) {
        let unique_pred = pred_of(b).len() == 1;
        for k in start[b]..start[b] + live_len[b] as usize {
            let phi = insts[k];
            if LLVMGetInstructionOpcode(phi) != LLVMOpcode::LLVMPHI {
                break;
            }
            vals.clear();
            let mut lost = false;
            for e in 0..LLVMCountIncoming(phi) {
                let p = bidx[&LLVMGetIncomingBlock(phi, e)] as usize;
                if reach[p] && has_edge(p, b as u32) {
                    vals.push(LLVMGetIncomingValue(phi, e));
                } else {
                    lost = true;
                }
            }
            if vals.is_empty() {
                continue;
            }
            if unique_pred {
                let v = vals[0];
                alias.insert(phi, if v == phi { std::ptr::null_mut() } else { v });
            } else if lost {
                let mut c = vals[0];
                let mut agree = true;
                for &v in &vals[1..] {
                    if v != c && v != phi {
                        if c != phi {
                            agree = false;
                            break;
                        }
                        c = v;
                    }
                }
                if agree {
                    alias.insert(phi, if c == phi { std::ptr::null_mut() } else { c });
                }
            }
        }
    }
    let rep = |mut v: LLVMValueRef| -> LLVMValueRef {
        for _ in 0..64 {
            match alias.get(&v) {
                Some(&n) if !n.is_null() => v = n,
                Some(_) => return std::ptr::null_mut(),
                None => return v,
            }
        }
        v
    };

    // ---- GC values: arguments, then surviving instructions.
    let mut id_of: PtrMap<LLVMValueRef, u32> = PtrMap::default();
    let mut def_block: Vec<u32> = Vec::new();
    let mut def_pos: Vec<i64> = Vec::new();
    let mut maybe_derived: Vec<bool> = Vec::new();
    for p in 0..LLVMCountParams(function) {
        let a = LLVMGetParam(function, p);
        if is_gc_type(LLVMTypeOf(a)) {
            id_of.insert(a, def_block.len() as u32);
            def_block.push(0);
            def_pos.push(-1);
            maybe_derived.push(false);
        }
    }
    let mut phi_or_select: Vec<u32> = Vec::new();
    for b in (0..nb).filter(|&b| reach[b]) {
        for k in 0..live_len[b] as usize {
            let i = insts[start[b] + k];
            if !is_gc_type(LLVMTypeOf(i)) || alias.contains_key(&i) {
                continue;
            }
            let id = def_block.len() as u32;
            id_of.insert(i, id);
            def_block.push(b as u32);
            def_pos.push(k as i64);
            let op = LLVMGetInstructionOpcode(i);
            let derived = matches!(
                op,
                LLVMOpcode::LLVMGetElementPtr
                    | LLVMOpcode::LLVMAddrSpaceCast
                    | LLVMOpcode::LLVMBitCast
                    | LLVMOpcode::LLVMFreeze
            );
            if derived {
                out.derived_values += 1;
            }
            if matches!(op, LLVMOpcode::LLVMPHI | LLVMOpcode::LLVMSelect) {
                phi_or_select.push(id);
            }
            maybe_derived.push(derived);
        }
    }
    let nv = def_block.len();
    // RS4GC's base pointers (`findBasePointer`) for phis and selects.
    let base = if phi_or_select.is_empty() {
        Vec::new()
    } else {
        phi_select_bases(
            &phi_or_select,
            nv,
            &insts,
            &start,
            &def_block,
            &def_pos,
            &id_of,
            &bidx,
            &reach,
            &|p, b| has_edge(p, b),
            &rep,
        )
    };
    // How many relocations one crossing of each value costs: 0 when its base
    // is a constant (RS4GC drops it from the live set), 2 when RS4GC gives it
    // a fresh base phi/select (live exactly where it is), else 1. A value
    // whose base is some *other* existing value costs 1 plus at most 1 more
    // (that base may already be live): it is counted in the bound.
    let mut weight = vec![1u8; nv];
    for (v, kind) in base.iter().enumerate() {
        match kind {
            BaseKind::Own => {}
            BaseKind::Constant => weight[v] = 0,
            BaseKind::Fresh => weight[v] = 2,
            BaseKind::Existing => maybe_derived[v] = true,
        }
    }
    out.fresh_bases = base.iter().filter(|k| **k == BaseKind::Fresh).count();
    out.gc_values = nv;

    // ---- the `icmp` move: a single-use icmp feeding a surviving conditional
    // branch is moved to just before that branch.
    let mut moved: PtrMap<LLVMValueRef, (u32, u32)> = PtrMap::default();
    if !cond_branches.is_empty() {
        let mut uses: PtrMap<LLVMValueRef, u32> =
            cond_branches.iter().map(|&(_, c)| (c, 0)).collect();
        for b in (0..nb).filter(|&b| reach[b]) {
            for k in 0..live_len[b] as usize {
                let i = insts[start[b] + k];
                if LLVMGetInstructionOpcode(i) == LLVMOpcode::LLVMPHI {
                    if alias.contains_key(&i) {
                        continue;
                    }
                    for e in 0..LLVMCountIncoming(i) {
                        let p = bidx[&LLVMGetIncomingBlock(i, e)] as usize;
                        if reach[p] && has_edge(p, b as u32) {
                            if let Some(n) = uses.get_mut(&LLVMGetIncomingValue(i, e)) {
                                *n += 1;
                            }
                        }
                    }
                    continue;
                }
                for o in 0..LLVMGetNumOperands(i) as u32 {
                    if let Some(n) = uses.get_mut(&LLVMGetOperand(i, o)) {
                        *n += 1;
                    }
                }
            }
        }
        for &(b, c) in &cond_branches {
            if uses.get(&c) == Some(&1) {
                moved.insert(c, (b, live_len[b as usize] - 1));
            }
        }
    }

    // ---- uses of GC values: (value, block, position or LIVE_OUT).
    let mut uses: Vec<(u32, u32, u32)> = Vec::new();
    for b in (0..nb).filter(|&b| reach[b]) {
        for k in 0..live_len[b] as usize {
            let i = insts[start[b] + k];
            if LLVMGetInstructionOpcode(i) == LLVMOpcode::LLVMPHI {
                if alias.contains_key(&i) || !is_gc_type(LLVMTypeOf(i)) {
                    continue;
                }
                for e in 0..LLVMCountIncoming(i) {
                    let p = bidx[&LLVMGetIncomingBlock(i, e)] as usize;
                    if !(reach[p] && has_edge(p, b as u32)) {
                        continue;
                    }
                    let v = rep(LLVMGetIncomingValue(i, e));
                    if let Some(&id) = id_of.get(&v) {
                        uses.push((id, p as u32, LIVE_OUT));
                    }
                }
                continue;
            }
            if deleted.contains_key(&i)
                || (k + 1 == live_len[b] as usize && fate[b] == InvokeFate::Erased)
            {
                continue;
            }
            let (ub, up) = moved.get(&i).copied().unwrap_or((b as u32, k as u32));
            for o in 0..LLVMGetNumOperands(i) as u32 {
                let v = LLVMGetOperand(i, o);
                if v.is_null() || !is_gc_type(LLVMTypeOf(v)) {
                    continue;
                }
                if let Some(&id) = id_of.get(&rep(v)) {
                    uses.push((id, ub, up));
                }
            }
        }
    }
    // CSR by value.
    let mut use_start = vec![0usize; nv + 1];
    for &(v, _, _) in &uses {
        use_start[v as usize + 1] += 1;
    }
    for v in 0..nv {
        use_start[v + 1] += use_start[v];
    }
    let mut by_value = vec![(0u32, 0u32); uses.len()];
    let mut fill = use_start.clone();
    for &(v, b, p) in &uses {
        by_value[fill[v as usize]] = (b, p);
        fill[v as usize] += 1;
    }
    drop(uses);

    // ---- safepoints, CSR by block, positions ascending.
    let mut sp_start = vec![0usize; nb + 1];
    let mut sp_pos: Vec<u32> = Vec::new();
    for b in 0..nb {
        sp_start[b] = sp_pos.len();
        if !reach[b] {
            continue;
        }
        let n = live_len[b] as usize;
        for k in 0..n {
            let i = insts[start[b] + k];
            let edges = match LLVMGetInstructionOpcode(i) {
                LLVMOpcode::LLVMCall | LLVMOpcode::LLVMCallBr => 1u8,
                LLVMOpcode::LLVMInvoke => match fate[b] {
                    InvokeFate::Edges(e) if k + 1 == n => e,
                    _ => continue,
                },
                _ => continue,
            };
            if needs_statepoint(i) {
                sp_pos.push(k as u32);
                out.safepoints.push(SafepointLiveness {
                    block: b as u32,
                    index: k as u32,
                    edges,
                    live: 0,
                });
            }
        }
    }
    sp_start[nb] = sp_pos.len();
    let nsp = sp_pos.len();
    if nsp == 0 {
        return out;
    }

    // ---- per-value backward liveness walk, accumulated into difference
    // arrays over the global safepoint order.
    let mut diff = vec![0i64; nsp + 1];
    let mut derived_diff = if out.derived_values > 0 {
        vec![0i64; nsp + 1]
    } else {
        Vec::new()
    };
    let mut in_stamp = vec![0u32; nb];
    let mut out_stamp = vec![0u32; nb];
    let mut lu_stamp = vec![0u32; nb];
    let mut lu = vec![0u32; nb];
    let mut work: Vec<u32> = Vec::new();
    let mut in_blocks: Vec<u32> = Vec::new();
    let mut steps = 0u64;
    for v in 0..nv {
        let (u0, u1) = (use_start[v], use_start[v + 1]);
        if u0 == u1 {
            continue;
        }
        let stamp = v as u32 + 1;
        let defb = def_block[v] as usize;
        work.clear();
        in_blocks.clear();
        steps += (u1 - u0) as u64;
        let mut mark_in = |b: usize, work: &mut Vec<u32>, in_blocks: &mut Vec<u32>| {
            if in_stamp[b] != stamp {
                in_stamp[b] = stamp;
                work.push(b as u32);
                in_blocks.push(b as u32);
            }
        };
        for &(ub, up) in &by_value[u0..u1] {
            let ub = ub as usize;
            if up == LIVE_OUT {
                if out_stamp[ub] != stamp {
                    out_stamp[ub] = stamp;
                    if ub != defb {
                        mark_in(ub, &mut work, &mut in_blocks);
                    }
                }
            } else {
                if lu_stamp[ub] != stamp || up > lu[ub] {
                    lu_stamp[ub] = stamp;
                    lu[ub] = up;
                }
                if ub != defb {
                    mark_in(ub, &mut work, &mut in_blocks);
                }
            }
        }
        while let Some(b) = work.pop() {
            for &p in pred_of(b as usize) {
                steps += 1;
                let p = p as usize;
                if out_stamp[p] != stamp {
                    out_stamp[p] = stamp;
                    if p != defb {
                        mark_in(p, &mut work, &mut in_blocks);
                    }
                }
            }
        }
        steps += in_blocks.len() as u64 + 1;
        let derived = maybe_derived[v];
        let weight = i64::from(weight[v]);
        if weight == 0 {
            continue;
        }
        let mut add = |b: usize, from: i64, to: Option<u32>| {
            let sps = &sp_pos[sp_start[b]..sp_start[b + 1]];
            if sps.is_empty() {
                return;
            }
            let lo = if from < 0 {
                0
            } else {
                sps.partition_point(|&p| i64::from(p) <= from)
            };
            let hi = match to {
                None => sps.len(),
                // A use by the safepoint itself keeps the value live across
                // it: RS4GC takes the live set *into* the call, operands
                // included (`findLiveSetAtInst` walks the call itself).
                Some(end) => sps.partition_point(|&p| p <= end),
            };
            if hi > lo {
                let g = sp_start[b];
                diff[g + lo] += weight;
                diff[g + hi] -= weight;
                if derived {
                    derived_diff[g + lo] += 1;
                    derived_diff[g + hi] -= 1;
                }
            }
        };
        // Definition block: from the definition to the last use, or through
        // the end when the value is live out.
        if out_stamp[defb] == stamp {
            add(defb, def_pos[v], None);
        } else if lu_stamp[defb] == stamp {
            add(defb, def_pos[v], Some(lu[defb]));
        }
        for &b in &in_blocks {
            let b = b as usize;
            if out_stamp[b] == stamp {
                add(b, -1, None);
            } else if lu_stamp[b] == stamp {
                add(b, -1, Some(lu[b]));
            }
        }
    }
    out.work = steps;

    let mut live = 0i64;
    let mut dlive = 0i64;
    for (g, sp) in out.safepoints.iter_mut().enumerate() {
        live += diff[g];
        sp.live = live as u32;
        out.relocations += live as u64 * u64::from(sp.edges);
        if !derived_diff.is_empty() {
            dlive += derived_diff[g];
            out.derived_crossings += dlive as u64 * u64::from(sp.edges);
        }
    }
    out
}

/// RS4GC's base of a phi/select value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseKind {
    /// Its own base (every input is a base, or a `null`): relocated once.
    Own,
    /// All inputs are constants: the base is `null`, and RS4GC drops the
    /// value from the live set.
    Constant,
    /// Inputs disagree: RS4GC clones it into a `.base` phi/select that is
    /// live wherever it is.
    Fresh,
    /// Every input agrees on one other existing value.
    Existing,
}

/// One input of a phi/select, as `findBasePointer` sees it.
#[derive(Clone, Copy)]
enum BaseInput {
    /// The node itself.
    Myself,
    /// `null`: its own base, so it does not block pruning.
    Null,
    /// Any other constant: base `null`, but not its own BDV, so it blocks
    /// pruning (an `inttoptr` of a NaN-box tag, typically).
    Constant,
    /// A value that is its own base (call, load, argument, `inttoptr`...).
    Base(usize),
    /// A derived value: base elsewhere, and it blocks pruning.
    Derived(usize),
    /// Another phi/select, by node index.
    Node(u32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BaseState {
    Unknown,
    Base(usize),
    Conflict,
}

impl BaseState {
    fn meet(self, other: BaseState) -> BaseState {
        match (self, other) {
            (BaseState::Unknown, x) | (x, BaseState::Unknown) => x,
            (BaseState::Base(a), BaseState::Base(b)) if a == b => self,
            _ => BaseState::Conflict,
        }
    }
}

/// `findBasePointer` for every phi/select GC value: prune the nodes whose
/// inputs are all bases, then run the optimistic meet over the rest. Linear
/// in the phi/select inputs (each node's state changes at most twice).
#[allow(clippy::too_many_arguments)]
unsafe fn phi_select_bases(
    nodes: &[u32],
    nv: usize,
    insts: &[LLVMValueRef],
    start: &[usize],
    def_block: &[u32],
    def_pos: &[i64],
    id_of: &PtrMap<LLVMValueRef, u32>,
    bidx: &PtrMap<LLVMBasicBlockRef, u32>,
    reach: &[bool],
    has_edge: &dyn Fn(usize, u32) -> bool,
    rep: &dyn Fn(LLVMValueRef) -> LLVMValueRef,
) -> Vec<BaseKind> {
    let mut node_of = vec![u32::MAX; nv];
    for (n, &id) in nodes.iter().enumerate() {
        node_of[id as usize] = n as u32;
    }
    let classify = |me: u32, v: LLVMValueRef| -> BaseInput {
        if v.is_null() || LLVMIsConstant(v) != 0 {
            return if !v.is_null() && LLVMIsNull(v) != 0 {
                BaseInput::Null
            } else {
                BaseInput::Constant
            };
        }
        let Some(&id) = id_of.get(&v) else {
            // Not a GC value we track (it cannot be one: phi inputs share
            // the phi's type); treat it as a base of its own.
            return BaseInput::Base(v as usize);
        };
        let n = node_of[id as usize];
        if n == me {
            BaseInput::Myself
        } else if n != u32::MAX {
            BaseInput::Node(n)
        } else if !LLVMIsAInstruction(v).is_null()
            && matches!(
                LLVMGetInstructionOpcode(v),
                LLVMOpcode::LLVMGetElementPtr
                    | LLVMOpcode::LLVMAddrSpaceCast
                    | LLVMOpcode::LLVMBitCast
                    | LLVMOpcode::LLVMFreeze
            )
        {
            BaseInput::Derived(v as usize)
        } else {
            BaseInput::Base(v as usize)
        }
    };
    // Inputs, CSR by node, and users (for the worklists).
    let mut in_start = Vec::with_capacity(nodes.len() + 1);
    let mut inputs: Vec<BaseInput> = Vec::new();
    for (n, &id) in nodes.iter().enumerate() {
        in_start.push(inputs.len());
        let b = def_block[id as usize] as usize;
        let inst = insts[start[b] + def_pos[id as usize] as usize];
        if LLVMGetInstructionOpcode(inst) == LLVMOpcode::LLVMPHI {
            for e in 0..LLVMCountIncoming(inst) {
                let p = bidx[&LLVMGetIncomingBlock(inst, e)] as usize;
                if reach[p] && has_edge(p, b as u32) {
                    inputs.push(classify(n as u32, rep(LLVMGetIncomingValue(inst, e))));
                }
            }
        } else {
            for o in [1, 2] {
                inputs.push(classify(n as u32, rep(LLVMGetOperand(inst, o))));
            }
        }
    }
    in_start.push(inputs.len());
    let mut users: Vec<Vec<u32>> = vec![Vec::new(); nodes.len()];
    for n in 0..nodes.len() {
        for input in &inputs[in_start[n]..in_start[n + 1]] {
            if let BaseInput::Node(m) = *input {
                users[m as usize].push(n as u32);
            }
        }
    }

    // Pruning: a node is its own base when every input is itself, `null`,
    // a base, or an already-pruned node.
    let mut live = vec![true; nodes.len()];
    let prunable = |n: usize, live: &[bool]| {
        inputs[in_start[n]..in_start[n + 1]]
            .iter()
            .all(|input| match *input {
                BaseInput::Myself | BaseInput::Null | BaseInput::Base(_) => true,
                BaseInput::Node(m) => !live[m as usize],
                BaseInput::Constant | BaseInput::Derived(_) => false,
            })
    };
    let mut work: Vec<u32> = (0..nodes.len() as u32).collect();
    while let Some(n) = work.pop() {
        let n = n as usize;
        if live[n] && prunable(n, &live) {
            live[n] = false;
            work.extend(users[n].iter().copied().filter(|&u| live[u as usize]));
        }
    }

    // Optimistic meet over the nodes that remain.
    let mut state = vec![BaseState::Unknown; nodes.len()];
    let mut work: Vec<u32> = (0..nodes.len() as u32)
        .filter(|&n| live[n as usize])
        .collect();
    while let Some(n) = work.pop() {
        let n = n as usize;
        let mut s = BaseState::Unknown;
        for input in &inputs[in_start[n]..in_start[n + 1]] {
            let x = match *input {
                BaseInput::Myself => BaseState::Unknown,
                BaseInput::Null | BaseInput::Constant => BaseState::Base(0),
                BaseInput::Base(p) | BaseInput::Derived(p) => BaseState::Base(p),
                BaseInput::Node(m) if live[m as usize] => state[m as usize],
                // A pruned node is its own base. Odd keys cannot collide
                // with the (aligned) addresses of other bases, nor with 0.
                BaseInput::Node(m) => BaseState::Base(((nodes[m as usize] as usize + 1) << 1) | 1),
            };
            s = s.meet(x);
        }
        if s != state[n] {
            state[n] = s;
            work.extend(users[n].iter().copied().filter(|&u| live[u as usize]));
        }
    }

    let mut kind = vec![BaseKind::Own; nv];
    for (n, &id) in nodes.iter().enumerate() {
        if !live[n] {
            continue;
        }
        kind[id as usize] = match state[n] {
            BaseState::Unknown => BaseKind::Own,
            BaseState::Conflict => BaseKind::Fresh,
            BaseState::Base(0) => BaseKind::Constant,
            BaseState::Base(_) => BaseKind::Existing,
        };
    }
    kind
}

/// C library functions LLVM's `TargetLibraryInfo` recognizes (LLVM 22,
/// `llvm/include/llvm/Analysis/TargetLibraryInfo.td`, every
/// `TargetLibCall<"name", ...>`), sorted bytewise. RS4GC's
/// `callsGCLeafFunction` treats a call to any of them as a GC leaf. The
/// prototype check `TargetLibraryInfo` also applies is not repeated here: a
/// declaration with a library name but a foreign signature would be counted
/// as a leaf, which the audit would report as a mismatch.
#[rustfmt::skip]
const LIBFUNCS: &[&str] = &include!("gc_liveness_libfuncs.in");

#[cfg(test)]
#[path = "gc_liveness_tests.rs"]
mod tests;
