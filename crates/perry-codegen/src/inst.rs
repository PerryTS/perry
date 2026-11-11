//! Typed instruction storage for `LlBlock` (exp/llvm-inprocess, Phase 2b).
//!
//! Every variant renders byte-identically to the `format!` string the
//! corresponding `block.rs` semantic method used to emit — the migration
//! gate is the full unit suite (which pins exact rendered IR) plus
//! `PERRY_LLVM_INPROCESS=diff`'s object-byte verdict. `Raw` remains the
//! carrier for the `emit_raw` escape hatch (89 call sites of bespoke
//! shapes); typed variants are constructed natively by `dialect.rs` without
//! any per-line text, and the per-module raw-vs-typed counts are the
//! migration ratchet.
//!
//! Operands stay in the emitter's existing currency — register-name /
//! literal token strings — so lowering code is untouched by the migration;
//! what disappears is the line formatting and the line re-parsing.

use crate::types::LlvmType;

/// LLVM's `preserve_none` calling convention, textual token form (#8175).
///
/// Applied to recursion-participating specialized clones: with no
/// callee-saved registers there is nothing for a param-derived live-across-
/// call value to pin in the entry block, so LLVM shrink-wrapping can sink
/// the whole frame into the recursive path and the leaf path runs
/// frameless. The numeric C-API id lives in `dialect::LLVM_CC_PRESERVE_NONE`;
/// the two must name the same convention.
///
/// A call site whose convention disagrees with its callee is UB — every
/// consumer (define header, declare line, call, invoke) reads the ONE
/// module-level registry (`LlModule` -> `RegCounter`), and
/// `spec_preserve_none_tests` asserts define/call agreement module-wide.
pub(crate) const PRESERVE_NONE_CC: &str = "preserve_nonecc";

#[derive(Clone)]
pub enum LoadFlavor {
    Plain,
    Aligned(u32),
    Volatile,
    AtomicMonotonic(u32),
    /// `load atomic … unordered`: a monotone runtime latch (#11915). Atomic,
    /// so a concurrent writer is not a data race, but with no ordering, so the
    /// backend may fold it into a compare with memory and merge repeats.
    AtomicUnordered(u32),
    AtomicAcquire(u32),
    AtomicSeqCst(u32),
    /// `!invariant.load !0` tagged (issue #52).
    Invariant,
}

/// Metadata a branch carries for the compiler-output proofs.
///
/// `loop_tag` makes the branch a loop latch with an `!llvm.loop` loop ID. The
/// ID lists the loop's named region (`!{!"perry.region", !"<region id>"}`)
/// and, for the original copy of a byte-view-versioned loop, the
/// `!{!"perry.byte_view.fallback"}` operand. LLVM keeps unknown loop-ID
/// operands on the loop through rotation, unrolling and vectorization (the
/// vector body and the remainder both carry them), so the optimized IR still
/// names the loop. A loop ID is a numbered self-referencing node:
/// `LlModule::number_loop_metadata` numbers every tag once all functions are
/// lowered. An unnumbered tag renders no attachment, so the proofs fail
/// closed (the loop is not found), never open.
///
/// `byte_view_version` marks the admission branch of `stmt/byte_view_loop.rs`
/// (`!perry.byte_view.version !{}`): the one branch that chooses between the
/// native fast copy and the original copy of the loop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchMeta {
    pub loop_tag: Option<LoopTag>,
    pub byte_view_version: bool,
}

/// The loop ID of one latch (see [`BranchMeta`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoopTag {
    /// `FnCtx::region_id_for_label` of the loop's label.
    pub region: Option<String>,
    /// The original copy of a byte-view-versioned loop.
    pub byte_view_fallback: bool,
    /// Numbered loop-ID node; 0 = not numbered yet.
    pub id: u32,
}

impl LoopTag {
    /// The tag a loop's latch carries, if any: a labelled loop names its
    /// region, the original copy of a byte-view loop says so.
    pub fn for_loop(region: Option<String>, byte_view_fallback: bool) -> Option<Self> {
        (region.is_some() || byte_view_fallback).then_some(Self {
            region,
            byte_view_fallback,
            id: 0,
        })
    }
}

impl BranchMeta {
    fn render_into(&self, out: &mut String) {
        use std::fmt::Write;
        if let Some(tag) = &self.loop_tag {
            if tag.id != 0 {
                let _ = write!(out, ", !llvm.loop !{}", tag.id);
            }
        }
        if self.byte_view_version {
            out.push_str(", !perry.byte_view.version !{}");
        }
    }
}

/// `Clone` is what lets `root_reload.rs` carry a value's derivation RECIPE —
/// the root load plus the pure bit ops above it — to the stale use and
/// re-materialise it there (#7664). Cloning an instruction is cloning its
/// operand tokens; nothing in a variant is an identity.
#[derive(Clone)]
pub enum LlInst {
    /// Pre-rendered instruction line, two-space indent included.
    Raw(String),
    /// Binary op: `dst = <op> <pre><ty> a, b`. `pre` is the fast-math
    /// prefix for fp ops (`"reassoc contract "` etc., trailing space
    /// included) and `""` for integer/bit ops.
    Bin {
        dst: String,
        op: &'static str,
        pre: &'static str,
        ty: LlvmType,
        a: String,
        b: String,
    },
    FNeg {
        dst: String,
        pre: &'static str,
        a: String,
    },
    /// `dst = freeze <ty> a`: a poison operand becomes an arbitrary fixed
    /// value. Lets an `fptosi` round trip test exactness without UB.
    Freeze {
        dst: String,
        ty: LlvmType,
        a: String,
    },
    FCmp {
        dst: String,
        pred: String,
        /// Operand type. This USED to be hardcoded `double` at the render
        /// site, which silently produced invalid IR for a `float` operand
        /// (#10779 follow-up: `Buffer.readFloatLE` failed codegen with
        /// "'%r' defined with type 'float' but expected 'double'").
        ty: LlvmType,
        a: String,
        b: String,
    },
    ICmp {
        dst: String,
        pred: &'static str,
        ty: LlvmType,
        a: String,
        b: String,
    },
    Alloca {
        dst: String,
        ty: LlvmType,
    },
    Load {
        dst: String,
        ty: LlvmType,
        ptr: String,
        flavor: LoadFlavor,
    },
    Store {
        ty: LlvmType,
        val: String,
        ptr: String,
        volatile: bool,
        align: Option<u32>,
    },
    /// All `X to Y` conversions: bitcast, trunc/zext/sext, fp<->int, ptr<->int.
    Cast {
        dst: String,
        op: &'static str,
        from: LlvmType,
        v: String,
        to: LlvmType,
    },
    Select {
        dst: String,
        cond_ty: LlvmType,
        cond: String,
        ty: LlvmType,
        a: String,
        b: String,
    },
    /// Direct call `@callee` (name WITHOUT the `@`). `dst: None` renders the
    /// value-less form (`call void ...` and friends).
    Call {
        dst: Option<String>,
        ret: LlvmType,
        callee: String,
        args: Vec<(LlvmType, String)>,
        /// Explicit calling-convention token (`preserve_nonecc`), or `None`
        /// for the default C convention. Must match the callee's define
        /// header — a mismatch is UB, not a verifier error (#8175).
        cconv: Option<&'static str>,
        /// Call-site relocation exemption. The callee still participates in
        /// transitive effect analysis; this flag only controls caller-side
        /// root relocation and the emitted RS4GC attribute.
        gc_leaf: bool,
        /// Keep a cold selected-body call out of the caller.
        no_inline: bool,
    },
    /// Opaque-pointer indirect call. The callee operand itself is a `ptr`;
    /// argument types remain explicit at the call site.
    CallIndirect {
        dst: String,
        ret: LlvmType,
        fptr: String,
        args: Vec<(LlvmType, String)>,
        /// Call-site promise to RS4GC that no relocated value is observed
        /// after a collecting arm returns. This is narrower than a function
        /// effect: transitive leaf analysis must still treat the opaque target
        /// as collecting.
        gc_leaf: bool,
    },
    /// `call void asm sideeffect "", ""()` — the loop-preservation barrier.
    AsmBarrier,
    Br {
        label: String,
        meta: Option<Box<BranchMeta>>,
    },
    CondBr {
        cond: String,
        t: String,
        f: String,
        meta: Option<Box<BranchMeta>>,
    },
    Ret {
        ty: LlvmType,
        val: String,
    },
    RetVoid,
    Unreachable,
    Gep {
        dst: String,
        inbounds: bool,
        ty: LlvmType,
        ptr: String,
        idxs: Vec<(LlvmType, String)>,
    },
    Phi {
        dst: String,
        ty: LlvmType,
        /// (value, block-label-without-%) pairs.
        pairs: Vec<(String, String)>,
    },
}

impl LlInst {
    /// Append the rendered line (no trailing newline) to `out`. Must stay
    /// byte-identical to the `format!` the emitting method used — the unit
    /// suite pins this.
    pub fn render_into(&self, out: &mut String) {
        use std::fmt::Write;
        match self {
            LlInst::Raw(s) => out.push_str(s),
            LlInst::Bin {
                dst,
                op,
                pre,
                ty,
                a,
                b,
            } => {
                let _ = write!(out, "  {dst} = {op} {pre}{ty} {a}, {b}");
            }
            LlInst::FNeg { dst, pre, a } => {
                let _ = write!(out, "  {dst} = fneg {pre}double {a}");
            }
            LlInst::Freeze { dst, ty, a } => {
                let _ = write!(out, "  {dst} = freeze {ty} {a}");
            }
            LlInst::FCmp {
                dst,
                pred,
                ty,
                a,
                b,
            } => {
                let _ = write!(out, "  {dst} = fcmp {pred} {ty} {a}, {b}");
            }
            LlInst::ICmp {
                dst,
                pred,
                ty,
                a,
                b,
            } => {
                let _ = write!(out, "  {dst} = icmp {pred} {ty} {a}, {b}");
            }
            LlInst::Alloca { dst, ty } => {
                let _ = write!(out, "  {dst} = alloca {ty}");
            }
            LlInst::Load {
                dst,
                ty,
                ptr,
                flavor,
            } => match flavor {
                LoadFlavor::Plain => {
                    let _ = write!(out, "  {dst} = load {ty}, ptr {ptr}");
                }
                LoadFlavor::Aligned(n) => {
                    let _ = write!(out, "  {dst} = load {ty}, ptr {ptr}, align {n}");
                }
                LoadFlavor::Volatile => {
                    let _ = write!(out, "  {dst} = load volatile {ty}, ptr {ptr}");
                }
                LoadFlavor::AtomicMonotonic(n) => {
                    let _ = write!(
                        out,
                        "  {dst} = load atomic {ty}, ptr {ptr} monotonic, align {n}"
                    );
                }
                LoadFlavor::AtomicAcquire(n) => {
                    let _ = write!(
                        out,
                        "  {dst} = load atomic {ty}, ptr {ptr} acquire, align {n}"
                    );
                }
                LoadFlavor::AtomicUnordered(n) => {
                    let _ = write!(
                        out,
                        "  {dst} = load atomic {ty}, ptr {ptr} unordered, align {n}"
                    );
                }
                LoadFlavor::AtomicSeqCst(n) => {
                    let _ = write!(
                        out,
                        "  {dst} = load atomic {ty}, ptr {ptr} seq_cst, align {n}"
                    );
                }
                LoadFlavor::Invariant => {
                    let _ = write!(out, "  {dst} = load {ty}, ptr {ptr}, !invariant.load !0");
                }
            },
            LlInst::Store {
                ty,
                val,
                ptr,
                volatile,
                align,
            } => {
                if *volatile {
                    let _ = write!(out, "  store volatile {ty} {val}, ptr {ptr}");
                } else {
                    let _ = write!(out, "  store {ty} {val}, ptr {ptr}");
                }
                if let Some(n) = align {
                    let _ = write!(out, ", align {n}");
                }
            }
            LlInst::Cast {
                dst,
                op,
                from,
                v,
                to,
            } => {
                let _ = write!(out, "  {dst} = {op} {from} {v} to {to}");
            }
            LlInst::Select {
                dst,
                cond_ty,
                cond,
                ty,
                a,
                b,
            } => {
                let _ = write!(out, "  {dst} = select {cond_ty} {cond}, {ty} {a}, {ty} {b}");
            }
            LlInst::Call {
                dst,
                ret,
                callee,
                args,
                cconv,
                gc_leaf,
                no_inline,
            } => {
                out.push_str("  ");
                if let Some(d) = dst {
                    let _ = write!(out, "{d} = ");
                }
                match cconv {
                    Some(cc) => {
                        let _ = write!(out, "call {cc} {ret} @{callee}(");
                    }
                    None => {
                        let _ = write!(out, "call {ret} @{callee}(");
                    }
                }
                push_args(out, args);
                out.push(')');
                if *no_inline {
                    out.push_str(" noinline");
                }
                if *gc_leaf {
                    out.push_str(" \"gc-leaf-function\"");
                }
            }
            LlInst::CallIndirect {
                dst,
                ret,
                fptr,
                args,
                gc_leaf,
            } => {
                let _ = write!(out, "  {dst} = call {ret} {fptr}(");
                push_args(out, args);
                out.push(')');
                if *gc_leaf {
                    out.push_str(" \"gc-leaf-function\"");
                }
            }
            LlInst::AsmBarrier => {
                // `"gc-leaf-function"` exempts the barrier from
                // rewrite-statepoints-for-gc: RS4GC otherwise wraps the call
                // into a `gc.statepoint` whose callee is the inline asm —
                // invalid IR ("Cannot take the address of an inline asm!")
                // that SIGBUSes in ISel because the in-process pipeline does
                // not re-verify (#8082). An empty asm can never reach a
                // safepoint, so the exemption is sound by construction.
                out.push_str("  call void asm sideeffect \"\", \"\"() \"gc-leaf-function\"");
            }
            LlInst::Br { label, meta } => {
                let _ = write!(out, "  br label %{label}");
                if let Some(meta) = meta {
                    meta.render_into(out);
                }
            }
            LlInst::CondBr { cond, t, f, meta } => {
                let _ = write!(out, "  br i1 {cond}, label %{t}, label %{f}");
                if let Some(meta) = meta {
                    meta.render_into(out);
                }
            }
            LlInst::Ret { ty, val } => {
                let _ = write!(out, "  ret {ty} {val}");
            }
            LlInst::RetVoid => out.push_str("  ret void"),
            LlInst::Unreachable => out.push_str("  unreachable"),
            LlInst::Gep {
                dst,
                inbounds,
                ty,
                ptr,
                idxs,
            } => {
                let kw = if *inbounds {
                    "getelementptr inbounds"
                } else {
                    "getelementptr"
                };
                let _ = write!(out, "  {dst} = {kw} {ty}, ptr {ptr}, ");
                push_args(out, idxs);
            }
            LlInst::Phi { dst, ty, pairs } => {
                let _ = write!(out, "  {dst} = phi {ty} ");
                for (i, (v, l)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let _ = write!(out, "[ {v}, %{l} ]");
                }
            }
        }
    }

    /// Rendered byte length, excluding the trailing newline. Exact — the
    /// codegen-unit balancer (#5391) partitions on these sums, and an
    /// estimate that drifted would change the DEFAULT path's unit split.
    pub fn text_len(&self) -> usize {
        match self {
            LlInst::Raw(s) => s.len(),
            _ => {
                thread_local! {
                    static SCRATCH: std::cell::RefCell<String> =
                        std::cell::RefCell::new(String::with_capacity(128));
                }
                SCRATCH.with(|s| {
                    let mut s = s.borrow_mut();
                    s.clear();
                    self.render_into(&mut s);
                    s.len()
                })
            }
        }
    }

    /// The rendered line for text-scanning consumers that only make sense
    /// on raw lines. Typed variants answer predicates structurally
    /// (see `LlBlock::contains_gc_unsafe_call`).
    pub fn scan_str(&self) -> &str {
        match self {
            LlInst::Raw(s) => s,
            _ => "",
        }
    }

    /// True if this is a call into the perry runtime (anything that could
    /// allocate / trigger GC): not an `@llvm.*` intrinsic, not the inline-asm
    /// barrier. Mirrors the historical string scan in
    /// `contains_gc_unsafe_call` exactly.
    pub fn is_gc_unsafe_call(&self) -> bool {
        match self {
            LlInst::Raw(s) => {
                let Some(pos) = s.find("call ") else {
                    return false;
                };
                let callee = &s[pos..];
                !callee.contains("@llvm.")
                    && !callee.contains(" asm ")
                    // `js_shadow_slot_set` is a bounds-checked TLS store
                    // (`gc/roots/shadow_stack.rs`): it cannot allocate,
                    // collect, or revoke a layout, which is precisely what the
                    // two call-free clone scans exist to exclude. Without the
                    // exemption a proven-numeric accumulator's per-statement
                    // shadow CLEAR — itself emitted BECAUSE the value is a
                    // known non-pointer — condemned the whole fast clone.
                    && !callee.contains("@js_shadow_slot_set")
            }
            LlInst::Call { callee, .. } => {
                !callee.starts_with("llvm.") && callee != "js_shadow_slot_set"
            }
            LlInst::CallIndirect { .. } => true,
            _ => false,
        }
    }
}

fn push_args(out: &mut String, args: &[(LlvmType, String)]) {
    use std::fmt::Write;
    for (i, (t, v)) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{t} {v}");
    }
}

impl LlInst {
    pub(crate) fn relocate_callable(&mut self, r: &crate::codegen::callable_reloc::Relocations) {
        use LlInst::*;
        match self {
            Raw(s) => r.text(s),
            Bin { a, b, .. } | FCmp { a, b, .. } | ICmp { a, b, .. } => {
                r.text(a);
                r.text(b);
            }
            FNeg { a, .. } | Freeze { a, .. } => r.text(a),
            Load { ptr, .. } => r.text(ptr),
            Store { val, ptr, .. } => {
                r.text(val);
                r.text(ptr);
            }
            Cast { v, .. } => r.text(v),
            Select { cond, a, b, .. } => {
                r.text(cond);
                r.text(a);
                r.text(b);
            }
            Call { callee, args, .. } => {
                r.symbol(callee);
                for (_, arg) in args {
                    r.text(arg);
                }
            }
            CallIndirect { fptr, args, .. } => {
                r.text(fptr);
                for (_, arg) in args {
                    r.text(arg);
                }
            }
            CondBr { cond, .. } => r.text(cond),
            Ret { val, .. } => r.text(val),
            Gep { ptr, idxs, .. } => {
                r.text(ptr);
                for (_, idx) in idxs {
                    r.text(idx);
                }
            }
            Phi { pairs, .. } => {
                for (val, _) in pairs {
                    r.text(val);
                }
            }
            Alloca { .. } | AsmBarrier | Br { .. } | RetVoid | Unreachable => {}
        }
    }
}
