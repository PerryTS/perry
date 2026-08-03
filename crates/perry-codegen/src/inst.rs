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

pub enum LoadFlavor {
    Plain,
    Aligned(u32),
    Volatile,
    AtomicSeqCst(u32),
    /// `!invariant.load !0` tagged (issue #52).
    Invariant,
}

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
    FCmp {
        dst: String,
        pred: String,
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
    },
    /// Pre-opaque-pointer indirect call, rendered `(T1, T2)*` exactly as the
    /// text emitter always has.
    CallIndirect {
        dst: String,
        ret: LlvmType,
        fptr: String,
        args: Vec<(LlvmType, String)>,
    },
    /// `call void asm sideeffect "", ""()` — the loop-preservation barrier.
    AsmBarrier,
    Br {
        label: String,
    },
    CondBr {
        cond: String,
        t: String,
        f: String,
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
            LlInst::FCmp { dst, pred, a, b } => {
                let _ = write!(out, "  {dst} = fcmp {pred} double {a}, {b}");
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
            } => {
                out.push_str("  ");
                if let Some(d) = dst {
                    let _ = write!(out, "{d} = ");
                }
                let _ = write!(out, "call {ret} @{callee}(");
                push_args(out, args);
                out.push(')');
            }
            LlInst::CallIndirect {
                dst,
                ret,
                fptr,
                args,
            } => {
                let _ = write!(out, "  {dst} = call {ret} (");
                for (i, (t, _)) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(t);
                }
                let _ = write!(out, ")* {fptr}(");
                push_args(out, args);
                out.push(')');
            }
            LlInst::AsmBarrier => {
                out.push_str("  call void asm sideeffect \"\", \"\"()");
            }
            LlInst::Br { label } => {
                let _ = write!(out, "  br label %{label}");
            }
            LlInst::CondBr { cond, t, f } => {
                let _ = write!(out, "  br i1 {cond}, label %{t}, label %{f}");
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
                !callee.contains("@llvm.") && !callee.contains(" asm ")
            }
            LlInst::Call { callee, .. } => !callee.starts_with("llvm."),
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
