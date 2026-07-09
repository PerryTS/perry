//! LLVM IR basic-block builder.
//!
//! Each method appends one textual LLVM IR instruction to an internal buffer;
//! `to_ir` produces the final text.
//!
//! We use `alloca` + `load`/`store` for locals and rely on LLVM's `mem2reg`
//! pass (run automatically by `clang -O2` or higher) to promote them to SSA
//! form — locals just become stack slots at codegen time and LLVM's optimizer
//! sorts out the registers. Explicit `phi` nodes are still emitted for
//! control-flow merges (if/else value context, short-circuit logical ops).

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::codegen::FpContractMode;
use crate::types::LlvmType;

/// Per-build floating-point flags captured by each function/block builder.
///
/// These are intentionally explicit instance state. Older code used process-
/// global atomics set at `compile_module` entry, which was only sound while
/// every parallel module in a build shared identical FP options. Tests and
/// embedding callers can compile modules with different options concurrently,
/// so f64 emitters must derive their FMF prefix from the owning block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FpFlags {
    fast_math: bool,
    fp_contract_mode: FpContractMode,
}

impl FpFlags {
    pub fn new(fast_math: bool, fp_contract_mode: FpContractMode) -> Self {
        Self {
            fast_math,
            fp_contract_mode,
        }
    }

    /// Inserted between the opcode and type in fp instructions:
    /// `fadd reassoc contract double …` vs `fadd double …`.
    fn fmf_prefix(self) -> &'static str {
        match (self.fast_math, self.fp_contract_mode.permits_contract()) {
            (false, false) => "",
            (false, true) => "contract ",
            (true, false) => "reassoc ",
            (true, true) => "reassoc contract ",
        }
    }
}

impl Default for FpFlags {
    fn default() -> Self {
        Self::new(false, FpContractMode::Off)
    }
}

/// Function-wide register counter shared between all blocks in a function.
///
/// Registers are `%r1`, `%r2`, … unique across the entire function body —
/// LLVM requires SSA value names to be unique per function, not per block.
#[derive(Default)]
pub struct RegCounter {
    value: Cell<u32>,
}

impl RegCounter {
    pub fn new() -> Self {
        Self {
            value: Cell::new(0),
        }
    }

    pub fn next(&self) -> u32 {
        let v = self.value.get() + 1;
        self.value.set(v);
        v
    }
}

pub struct LlBlock {
    pub label: String,
    instructions: Vec<String>,
    terminated: bool,
    counter: Rc<RegCounter>,
    fp_flags: FpFlags,
    /// Provenance of nan-box bitcast results emitted in THIS block, for
    /// round-trip folding (#5334 lever C). A nan-boxed JS value is an `f64`,
    /// so any bit-level work unboxes (`bitcast double -> i64`) and any value
    /// production re-boxes (`bitcast i64 -> double`). When a value is boxed
    /// then immediately unboxed again (or vice versa) the pair is identity:
    /// `bitcast(bitcast(x)) == x`. `clang -O1+` folds these via instcombine,
    /// but the oversized-module path forces `-O0` (see #4880), which does not
    /// — so we fold at emit time. Maps a bitcast RESULT reg to its
    /// pre-bitcast source of the OPPOSITE type, scoped to this block so the
    /// source is guaranteed to dominate (SSA, emitted earlier in the same
    /// block).
    i64_from_double: HashMap<String, String>,
    double_from_i64: HashMap<String, String>,
}

impl LlBlock {
    pub fn new(label: impl Into<String>, counter: Rc<RegCounter>) -> Self {
        Self::new_with_fp_flags(label, counter, FpFlags::default())
    }

    pub fn new_with_fp_flags(
        label: impl Into<String>,
        counter: Rc<RegCounter>,
        fp_flags: FpFlags,
    ) -> Self {
        Self {
            label: label.into(),
            instructions: Vec::new(),
            terminated: false,
            counter,
            fp_flags,
            i64_from_double: HashMap::new(),
            double_from_i64: HashMap::new(),
        }
    }

    pub fn is_terminated(&self) -> bool {
        self.terminated
    }

    /// #5093: true if this block contains a `call` to anything other than an
    /// `@llvm.*` intrinsic or an inline-asm marker. The class-field versioned
    /// loop uses this to verify AT COMPILE TIME that its fast clone came out
    /// call-free (no runtime call ⇒ no allocation ⇒ no GC ⇒ the
    /// preheader-cached receiver pointer cannot move and the hoisted shape
    /// check cannot be invalidated mid-loop). Intrinsic/libm-style calls
    /// never enter the perry runtime, so they cannot trigger a collection.
    pub fn contains_gc_unsafe_call(&self) -> bool {
        self.instructions.iter().any(|line| {
            let Some(pos) = line.find("call ") else {
                return false;
            };
            let callee = &line[pos..];
            !callee.contains("@llvm.") && !callee.contains(" asm ")
        })
    }

    /// Allocate a fresh SSA register name in the enclosing function's
    /// virtual register pool (e.g. `"%r42"`). Safe to call between
    /// `gep` / other instructions that may emit sub-registers. Pair with
    /// `emit_raw` when you need a custom instruction whose type string
    /// isn't in the `LlvmType` alphabet (e.g. a literal `[N x i32]`
    /// array type passed to `getelementptr`).
    pub fn fresh_reg(&mut self) -> String {
        self.reg()
    }

    fn emit(&mut self, line: impl Into<String>) {
        // Never emit instructions after a terminator — LLVM rejects them and
        // the symptom is a confusing `clang` parse error many lines later.
        // We silently drop them: this mirrors a common bug pattern in anvil
        // where catch-all statement visitors occasionally fall through past
        // an already-emitted `ret`/`br`.
        if self.terminated {
            return;
        }
        self.instructions.push(format!("  {}", line.into()));
    }

    fn reg(&self) -> String {
        format!("%r{}", self.counter.next())
    }

    pub fn next_reg(&self) -> String {
        self.reg()
    }

    pub fn emit_raw(&mut self, line: impl Into<String>) {
        self.emit(line);
    }

    /// Number of instructions currently in this block. Used by
    /// `LlFunction::mark_entry_init_boundary` to record where the entry
    /// block's "prelude" (init calls) ends so post-init hoisted setup
    /// (e.g. cached global loads) can be spliced in at exactly that
    /// point — after the inits run but before user code, so the load
    /// dominates every use yet sees the up-to-date global value.
    pub fn instruction_count(&self) -> usize {
        self.instructions.len()
    }

    /// Iterate over the raw instruction strings (each already prefixed
    /// with two leading spaces, no trailing newline). Used by
    /// `LlFunction::to_ir` when it needs to splice hoisted setup into
    /// the entry block at a specific boundary.
    pub fn instructions_iter(&self) -> impl Iterator<Item = &str> {
        self.instructions.iter().map(|s| s.as_str())
    }

    // -------- Arithmetic (double) --------
    //
    // FP ops are emitted with no LLVM fast-math flags by default. Setting
    // `FpFlags::fast_math = true` (driven by the `--fast-math` CLI flag,
    // `PERRY_FAST_MATH=1` env var, or `perry.fastMath` in package.json)
    // adds `reassoc`; setting `fp_contract_mode` to `on` or `fast`
    // adds `contract` to every fadd/fsub/fmul/fdiv/frem/fneg.
    //
    // What the two independent flags actually buy:
    //   - `reassoc`: lets LLVM reorder `(a + b) + c → a + (b + c)`, which
    //     is what the loop-vectorizer needs to break a serial accumulator
    //     chain into 4 parallel accumulators. The win is real (and large)
    //     on tight `sum += constant` loops — measured ~7x on M-series
    //     ARM64. On data-dependent reductions (`sum += xs[i]`), the
    //     measured win is ~0% (LLVM can't fully vectorize, and Node
    //     vectorizes too where it can).
    //   - `contract`: allow fused multiply-add (FMA). On dot-product
    //     style code (`a*b + c`), allows fusing into a single FMA
    //     instruction with a single rounding step. Measured ~0% effect
    //     on M-series ARM64 in our benchmarks (FMA latency matches
    //     fmul+fadd here; Node also emits FMA where it can).
    //
    // What the two flags break: ECMAScript bit-exact f64 semantics. With
    // both on, ~30% of randomly-generated FP programs diverge from Node
    // by 1 ULP. Examples: `(a/b) * b` gets rewritten to `(a*b) / b`, and
    // `a*b + c` becomes a fused FMA. Without them, ~6% still diverge
    // (residual from the LLVM SLP vectorizer at -O3, not gated by these
    // per-instruction flags). Default is OFF so the bit-exact case is
    // the user's default experience.
    //
    // We deliberately DON'T emit the full `fast` flag set (`nnan ninf nsz
    // arcp contract afn reassoc`) even when fast-math is on. `nnan` and
    // `ninf` in particular are UB-style flags — they tell LLVM to assume
    // no NaN or Inf inputs, which is catastrophic for Perry: NaN-boxing
    // uses NaN bit patterns for EVERY non-number value (strings, objects,
    // null, undefined, booleans). Passing `-ffast-math` to clang was
    // tried briefly at v0.2-era commit 083ce16 and reverted two days
    // later in b5a8c83f because `-ffinite-math-only` (implied by
    // `-ffast-math`) made LLVM replace TAG_NULL / TAG_UNDEFINED
    // constants with 0.0 at codegen time. The clang step passes
    // `-fno-math-errno` only — every fast-math effect in Perry comes
    // from the per-instruction FMFs emitted here.

    pub fn fadd(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = fadd {}double {}, {}",
            r,
            self.fp_flags.fmf_prefix(),
            a,
            b
        ));
        r
    }

    pub fn fsub(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = fsub {}double {}, {}",
            r,
            self.fp_flags.fmf_prefix(),
            a,
            b
        ));
        r
    }

    pub fn fmul(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = fmul {}double {}, {}",
            r,
            self.fp_flags.fmf_prefix(),
            a,
            b
        ));
        r
    }

    pub fn fdiv(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = fdiv {}double {}, {}",
            r,
            self.fp_flags.fmf_prefix(),
            a,
            b
        ));
        r
    }

    pub fn frem(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = frem {}double {}, {}",
            r,
            self.fp_flags.fmf_prefix(),
            a,
            b
        ));
        r
    }

    pub fn fneg(&mut self, a: &str) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = fneg {}double {}",
            r,
            self.fp_flags.fmf_prefix(),
            a
        ));
        r
    }

    // -------- Comparisons --------

    /// Float comparison. `cond` is an LLVM predicate string: `olt`, `ole`,
    /// `ogt`, `oge`, `oeq`, `one`, `ord`, `uno`, …
    pub fn fcmp(&mut self, cond: &str, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fcmp {} double {}, {}", r, cond, a, b));
        r
    }

    pub fn icmp_eq(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp eq {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_ne(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp ne {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_slt(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp slt {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_sgt(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp sgt {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_sle(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp sle {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_ult(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp ult {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_ule(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp ule {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_ugt(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp ugt {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_sge(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp sge {} {}, {}", r, ty, a, b));
        r
    }

    // -------- Memory --------

    pub fn alloca(&mut self, ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = alloca {}", r, ty));
        r
    }

    pub fn load(&mut self, ty: LlvmType, ptr: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = load {}, ptr {}", r, ty, ptr));
        r
    }

    pub fn load_aligned(&mut self, ty: LlvmType, ptr: &str, alignment: u32) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = load {}, ptr {}, align {}",
            r, ty, ptr, alignment
        ));
        r
    }

    /// Load with `volatile` — prevents the optimizer from caching,
    /// reordering, or eliminating the load. Used for module globals
    /// that may be written by `optnone` functions.
    pub fn load_volatile(&mut self, ty: LlvmType, ptr: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = load volatile {}, ptr {}", r, ty, ptr));
        r
    }

    /// (Issue #52) Load tagged with `!invariant.load !0`. LLVM's GVN +
    /// LICM are allowed to hoist these loads out of any enclosing loop —
    /// the contract is that the loaded memory does not change between
    /// observable executions of the instruction. Use ONLY for values
    /// that are genuinely loop-invariant (e.g. a Buffer's `length`
    /// field, which stays pinned for the lifetime of the buffer since
    /// `Buffer.alloc(N)` never grows/shrinks).
    ///
    /// Misuse corrupts output silently: LLVM will cache the first
    /// value and reuse it across iterations even if the underlying
    /// memory changes.
    pub fn load_invariant(&mut self, ty: LlvmType, ptr: &str) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = load {}, ptr {}, !invariant.load !0",
            r, ty, ptr
        ));
        r
    }

    pub fn store(&mut self, ty: LlvmType, val: &str, ptr: &str) {
        self.emit(format!("store {} {}, ptr {}", ty, val, ptr));
    }

    pub fn store_aligned(&mut self, ty: LlvmType, val: &str, ptr: &str, alignment: u32) {
        self.emit(format!(
            "store {} {}, ptr {}, align {}",
            ty, val, ptr, alignment
        ));
    }

    /// Store with `volatile` — prevents optimizer from eliminating or
    /// reordering. Used for module globals.
    pub fn store_volatile(&mut self, ty: LlvmType, val: &str, ptr: &str) {
        self.emit(format!("store volatile {} {}, ptr {}", ty, val, ptr));
    }

    // -------- Conversions / bitcasts --------

    pub fn bitcast_i64_to_double(&mut self, val: &str) -> String {
        // #5334 lever C: fold `bitcast i64 (bitcast double %x to i64) to double`
        // back to %x — the round-trip is identity. The source dominates (same
        // block, emitted earlier), so reusing it is always valid SSA.
        if let Some(src) = self.i64_from_double.get(val) {
            return src.clone();
        }
        let r = self.reg();
        self.emit(format!("{} = bitcast i64 {} to double", r, val));
        if !self.terminated {
            self.double_from_i64.insert(r.clone(), val.to_string());
        }
        r
    }

    pub fn bitcast_double_to_i64(&mut self, val: &str) -> String {
        // #5334 lever C: fold `bitcast double (bitcast i64 %x to double) to i64`
        // back to %x — the round-trip is identity.
        if let Some(src) = self.double_from_i64.get(val) {
            return src.clone();
        }
        let r = self.reg();
        self.emit(format!("{} = bitcast double {} to i64", r, val));
        if !self.terminated {
            self.i64_from_double.insert(r.clone(), val.to_string());
        }
        r
    }

    pub fn sitofp(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = sitofp {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn uitofp(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = uitofp {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn fpext(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = fpext {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn fptrunc(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = fptrunc {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn fptosi(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = fptosi {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn fptoui(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = fptoui {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    /// Lean ToInt32 for compiler-internal coercions (array/element INDEXES,
    /// POD field narrowing): `fptosi` with a NaN/Infinity guard. NaN and
    /// ±Infinity produce 0 (per spec); normal values go through
    /// `fptosi(f64→i64) + trunc(i64→i32)`. Exact for every `|v| < 2^63`;
    /// finite values beyond that are poison — acceptable ONLY because index
    /// paths reject huge keys before element access (they route to the
    /// by-name/dynamic fallbacks) and these sites sit in structurally-gated
    /// hot loops that must stay lean. User-visible ToInt32 (`x | 0`,
    /// bitwise operands) must use [`Self::toint32_wrap`] instead.
    pub fn toint32(&mut self, val: &str) -> String {
        use crate::types::{DOUBLE, I1, I32, I64};
        let is_nan = self.fcmp("uno", val, "0.0");
        let fabs = self.call(DOUBLE, "llvm.fabs.f64", &[(DOUBLE, val)]);
        let is_inf = self.fcmp("oeq", &fabs, "0x7FF0000000000000");
        let is_bad = self.or(I1, &is_nan, &is_inf);
        let safe = self.select(I1, &is_bad, DOUBLE, "0.0", val);
        let as_i64 = self.fptosi(DOUBLE, &safe, I64);
        self.trunc(I64, &as_i64, I32)
    }

    /// ECMAScript ToInt32, exact for ALL inputs. NaN and ±Infinity produce 0
    /// (per spec); every finite value truncates toward zero and reduces
    /// modulo 2^32 before the two's-complement reinterpretation. The modular
    /// step matters: a bare `fptosi` is LLVM poison for finite `|v| >= 2^63`,
    /// so `(1e20) | 0` printed NaN instead of 1661992960 (CodeRabbit review
    /// on #5466; the hole predates the branch).
    ///
    /// Implemented as branchless integer exponent/mantissa manipulation on
    /// the f64 bits (the classic softfloat ToInt32): `value = ±mant ×
    /// 2^(bexp−1075)`, so shifting the 53-bit mantissa by the unbiased
    /// exponent yields the integer part mod 2^64, and the final `trunc`
    /// takes it mod 2^32. No `frem`/`fptosi` and no calls — an earlier
    /// `frem`-based reduction tripped the native-abi-proof structural gate
    /// on Linux (`unsupported_instruction` vectorization diagnostics) even
    /// though it was semantically right. Over-wide shifts are clamped
    /// (poison otherwise); every clamped case is mathematically 0 anyway.
    pub fn toint32_wrap(&mut self, val: &str) -> String {
        use crate::types::{I1, I32, I64};
        let bits = self.bitcast_double_to_i64(val);
        let exp_shifted = self.lshr(I64, &bits, "52");
        let bexp = self.and(I64, &exp_shifted, "2047");
        // NaN / ±Infinity (biased exponent 0x7FF) → 0.
        let is_bad = self.icmp_eq(I64, &bexp, "2047");
        // |v| < 1 (biased exponent < 1023: ±0, denormals, fractions) → 0.
        let too_small = self.icmp_ult(I64, &bexp, "1023");
        // 53-bit significand: fraction bits | implicit leading bit (2^52).
        let frac = self.and(I64, &bits, "4503599627370495");
        let mant = self.or(I64, &frac, "4503599627370496");
        let epos = self.sub(I64, &bexp, "1075");
        let eneg = self.sub(I64, "1075", &bexp);
        let shift_right = self.icmp_sgt(I64, &eneg, "0");
        let rsh_over = self.icmp_sgt(I64, &eneg, "63");
        let rsh_clamped = self.select(I1, &rsh_over, I64, "63", &eneg);
        let rsh_amt = self.select(I1, &shift_right, I64, &rsh_clamped, "0");
        // mant × 2^epos with epos ≥ 64 is ≡ 0 (mod 2^64); zeroed below.
        let lsh_huge = self.icmp_sgt(I64, &epos, "63");
        let lsh_clamped = self.select(I1, &lsh_huge, I64, "0", &epos);
        let lsh_amt = self.select(I1, &shift_right, I64, "0", &lsh_clamped);
        let rshifted = self.lshr(I64, &mant, &rsh_amt);
        let magnitude = self.shl(I64, &rshifted, &lsh_amt);
        let negative = self.icmp_slt(I64, &bits, "0");
        let negated = self.sub(I64, "0", &magnitude);
        let signed = self.select(I1, &negative, I64, &negated, &magnitude);
        let zero_bad = self.or(I1, &is_bad, &too_small);
        let zero_all = self.or(I1, &zero_bad, &lsh_huge);
        let wrapped = self.select(I1, &zero_all, I64, "0", &signed);
        self.trunc(I64, &wrapped, I32)
    }

    /// Fast ToInt32 — skip NaN/Infinity guards. Use ONLY when the input
    /// is known to be a finite number (e.g., result of integer arithmetic,
    /// `sitofp(i32)`, or a value that went through `toint32` already).
    pub fn toint32_fast(&mut self, val: &str) -> String {
        use crate::types::{I32, I64};
        let as_i64 = self.fptosi(crate::types::DOUBLE, val, I64);
        self.trunc(I64, &as_i64, I32)
    }

    pub fn trunc(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = trunc {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn zext(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = zext {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn sext(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = sext {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn inttoptr(&mut self, from_ty: LlvmType, val: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = inttoptr {} {} to ptr", r, from_ty, val));
        r
    }

    /// Load i32 from a NaN-unboxed pointer, with a null guard.
    /// If the pointer is < 4096 (null, TAG_UNDEFINED lower bits, or
    /// a small handle), returns 0 instead of dereferencing.
    /// Used for .length reads and bounds checks on arrays/strings.
    ///
    /// Uses `@perry_null_guard_zero` — a module-global i32 initialized
    /// to 0 that serves as a safe dereference target.
    ///
    /// (Issue #52) The length load is tagged `!invariant.load` — once
    /// resolved, an Array/Buffer's length field at offset 0 of the
    /// header is only mutated by in-place array-growth paths
    /// (IndexSet with realloc, `push`/`splice`). The tag lets LLVM's
    /// LICM hoist the load out of any read-only loop even when the
    /// intervening code contains calls the optimizer can't prove
    /// length-preserving. Writers (`IndexSet` slow path, `push`, etc.)
    /// use the plain `store`/`load` sequence on the same field, so
    /// they don't invalidate the invariant-tagged load *for this
    /// particular SSA value* — LLVM's memory SSA tracks the
    /// tag per-load, not per-address.
    pub fn safe_load_i32_from_ptr(&mut self, handle: &str) -> String {
        use crate::types::{I32, I64};
        let is_bad = self.icmp_ult(I64, handle, "4096");
        let handle_ptr = self.inttoptr(I64, handle);
        // Map bad pointers to a known-safe global that contains 0.
        let safe_ptr = {
            let r = self.reg();
            self.emit(format!(
                "{} = select i1 {}, ptr @perry_null_guard_zero, ptr {}",
                r, is_bad, handle_ptr
            ));
            r
        };
        // NOTE: must NOT use `!invariant.load` here. This helper is the
        // inline `.length` fast path for Arrays/Strings/Buffers, and
        // `arr.push`/`arr.pop`/`arr.shift`/`arr.unshift` all mutate the
        // u32 at offset 0. With `!invariant.load`, LLVM would forward an
        // earlier load past those calls (per LangRef: invariant.load tells
        // the optimizer the value never changes for the program's
        // lifetime, so loads after a potentially-modifying call get
        // replaced with the cached SSA value). User-visible: after
        // `arr.unshift(x); console.log(arr[0]); arr.shift();
        // console.log(arr.length)` the second length read returned the
        // pre-shift value because of this metadata.
        self.load(I32, &safe_ptr)
    }

    pub fn ptrtoint(&mut self, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = ptrtoint ptr {} to {}", r, val, to_ty));
        r
    }

    // -------- Integer arithmetic --------

    pub fn add(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = add {} {}, {}", r, ty, a, b));
        r
    }

    pub fn sub(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = sub {} {}, {}", r, ty, a, b));
        r
    }

    pub fn mul(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = mul {} {}, {}", r, ty, a, b));
        r
    }

    /// Signed integer remainder. Emitted by the `BinaryOp::Mod` integer
    /// fast path for `<int> % <int>` — avoids the libm `fmod()` call that
    /// `frem double` lowers to on ARM.
    pub fn srem(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = srem {} {}, {}", r, ty, a, b));
        r
    }

    /// Signed integer division. Reserved for future proof-guarded integer
    /// division paths; JS `/` currently lowers through double division.
    pub fn sdiv(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = sdiv {} {}, {}", r, ty, a, b));
        r
    }

    pub fn and(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = and {} {}, {}", r, ty, a, b));
        r
    }

    pub fn or(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = or {} {}, {}", r, ty, a, b));
        r
    }

    pub fn xor(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = xor {} {}, {}", r, ty, a, b));
        r
    }

    pub fn shl(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = shl {} {}, {}", r, ty, a, b));
        r
    }

    pub fn ashr(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = ashr {} {}, {}", r, ty, a, b));
        r
    }

    pub fn lshr(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = lshr {} {}, {}", r, ty, a, b));
        r
    }

    // -------- Select --------

    pub fn select(
        &mut self,
        cond_ty: LlvmType,
        cond: &str,
        ty: LlvmType,
        true_val: &str,
        false_val: &str,
    ) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = select {} {}, {} {}, {} {}",
            r, cond_ty, cond, ty, true_val, ty, false_val
        ));
        r
    }

    // -------- Function calls --------

    pub fn call(&mut self, ret_ty: LlvmType, func_name: &str, args: &[(LlvmType, &str)]) -> String {
        // #835 + #846: record this emission against the FFI provenance
        // registry. The driver consults the registry after all per-module
        // codegen finishes to auto-link the providing crate.
        crate::ext_registry::record_ffi_call(func_name);
        let r = self.reg();
        let arg_str = format_args(args);
        self.emit(format!(
            "{} = call {} @{}({})",
            r, ret_ty, func_name, arg_str
        ));
        r
    }

    pub fn call_void(&mut self, func_name: &str, args: &[(LlvmType, &str)]) {
        // #835 + #846: same registry hook as `call` — see comment there.
        crate::ext_registry::record_ffi_call(func_name);
        let arg_str = format_args(args);
        self.emit(format!("call void @{}({})", func_name, arg_str));
    }

    /// Empty inline-asm barrier (`call void asm sideeffect "", ""()`).
    /// Emits zero machine instructions but is opaque to the optimizer:
    /// LLVM's loop-deletion / IndVarSimplify cannot prove the surrounding
    /// loop has no observable effect, so the loop is preserved end-to-end
    /// instead of being folded to its closed-form result. Used by
    /// `lower_for` on bodies that would otherwise be eliminated (e.g.
    /// `for (let i=0;i<N;i++) sum+=1;` between two `Date.now()` calls —
    /// issue #74).
    pub fn asm_sideeffect_barrier(&mut self) {
        self.emit("call void asm sideeffect \"\", \"\"()".to_string());
    }

    pub fn call_indirect(
        &mut self,
        ret_ty: LlvmType,
        fn_ptr: &str,
        args: &[(LlvmType, &str)],
    ) -> String {
        let r = self.reg();
        let arg_str = format_args(args);
        let param_types: Vec<&str> = args.iter().map(|(t, _)| *t).collect();
        self.emit(format!(
            "{} = call {} ({})* {}({})",
            r,
            ret_ty,
            param_types.join(", "),
            fn_ptr,
            arg_str
        ));
        r
    }

    // -------- Control flow --------

    pub fn br(&mut self, target: &str) {
        self.emit(format!("br label %{}", target));
        self.terminated = true;
    }

    pub fn cond_br(&mut self, cond: &str, true_label: &str, false_label: &str) {
        self.emit(format!(
            "br i1 {}, label %{}, label %{}",
            cond, true_label, false_label
        ));
        self.terminated = true;
    }

    pub fn ret(&mut self, ty: LlvmType, val: &str) {
        self.emit(format!("ret {} {}", ty, val));
        self.terminated = true;
    }

    pub fn ret_void(&mut self) {
        self.emit("ret void");
        self.terminated = true;
    }

    pub fn unreachable(&mut self) {
        self.emit("unreachable");
        self.terminated = true;
    }

    // -------- GEP / Phi --------

    pub fn gep(&mut self, base_ty: LlvmType, ptr: &str, indices: &[(LlvmType, &str)]) -> String {
        let r = self.reg();
        let idx_str = indices
            .iter()
            .map(|(t, v)| format!("{} {}", t, v))
            .collect::<Vec<_>>()
            .join(", ");
        self.emit(format!(
            "{} = getelementptr {}, ptr {}, {}",
            r, base_ty, ptr, idx_str
        ));
        r
    }

    /// `getelementptr inbounds` — asserts the result stays within the
    /// allocation, enabling LLVM's SCEV and alias analysis to reason about
    /// the pointer provenance. Critical for loop vectorization: the
    /// LoopVectorizer refuses to auto-vectorize memory accesses through
    /// bare `inttoptr` because it can't identify the array bounds.
    pub fn gep_inbounds(
        &mut self,
        base_ty: LlvmType,
        ptr: &str,
        indices: &[(LlvmType, &str)],
    ) -> String {
        let r = self.reg();
        let idx_str = indices
            .iter()
            .map(|(t, v)| format!("{} {}", t, v))
            .collect::<Vec<_>>()
            .join(", ");
        self.emit(format!(
            "{} = getelementptr inbounds {}, ptr {}, {}",
            r, base_ty, ptr, idx_str
        ));
        r
    }

    pub fn phi(&mut self, ty: LlvmType, incoming: &[(&str, &str)]) -> String {
        let r = self.reg();
        let pairs = incoming
            .iter()
            .map(|(val, label)| format!("[ {}, %{} ]", val, label))
            .collect::<Vec<_>>()
            .join(", ");
        self.emit(format!("{} = phi {} {}", r, ty, pairs));
        r
    }

    pub fn to_ir(&self) -> String {
        let mut out = String::with_capacity(
            self.instructions.iter().map(|l| l.len() + 1).sum::<usize>() + self.label.len() + 2,
        );
        out.push_str(&self.label);
        out.push_str(":\n");
        out.push_str(&self.instructions.join("\n"));
        out
    }
}

fn format_args(args: &[(LlvmType, &str)]) -> String {
    args.iter()
        .map(|(t, v)| format!("{} {}", t, v))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DOUBLE, I64};
    use std::thread;

    fn fresh() -> LlBlock {
        LlBlock::new("entry.0", Rc::new(RegCounter::new()))
    }

    fn fresh_with(fast_math: bool, fp_contract_mode: FpContractMode) -> LlBlock {
        LlBlock::new_with_fp_flags(
            "entry.0",
            Rc::new(RegCounter::new()),
            FpFlags::new(fast_math, fp_contract_mode),
        )
    }

    #[test]
    fn nanbox_bitcast_roundtrip_folds_to_source() {
        // #5334 lever C: i64 -> double -> i64 collapses to the original i64,
        // and the reverse collapses to the original double. Only the first
        // bitcast of each pair is emitted.
        let mut b = fresh();
        let dbl = b.bitcast_i64_to_double("%arg"); // %r1 = bitcast i64 %arg to double
        let back = b.bitcast_double_to_i64(&dbl); // folds -> %arg (no new instr)
        assert_eq!(back, "%arg");

        let unboxed = b.bitcast_double_to_i64("%v"); // %r2 = bitcast double %v to i64
        let reboxed = b.bitcast_i64_to_double(&unboxed); // folds -> %v
        assert_eq!(reboxed, "%v");

        assert_eq!(
            b.to_ir(),
            "entry.0:\n  %r1 = bitcast i64 %arg to double\n  %r2 = bitcast double %v to i64"
        );
    }

    #[test]
    fn nanbox_bitcast_non_roundtrip_still_emits() {
        // A lone unbox with no inverse is untouched, and a fresh unbox of a
        // different value is not mistakenly folded.
        let mut b = fresh();
        let a = b.bitcast_double_to_i64("%a"); // %r1
        let c = b.bitcast_double_to_i64("%c"); // %r2 (different source, no fold)
        assert_eq!(a, "%r1");
        assert_eq!(c, "%r2");
        assert_eq!(
            b.to_ir(),
            "entry.0:\n  %r1 = bitcast double %a to i64\n  %r2 = bitcast double %c to i64"
        );
    }

    #[test]
    fn fadd_emits_expected_ir_default() {
        // Default mode: no fast-math FMF flags emitted, bit-exact with
        // Node.
        let mut b = fresh();
        let r = b.fadd("1.0", "2.0");
        assert_eq!(r, "%r1");
        assert_eq!(b.to_ir(), "entry.0:\n  %r1 = fadd double 1.0, 2.0");
    }

    #[test]
    fn fadd_emits_contract_when_fp_contract_on() {
        let mut b = fresh_with(false, FpContractMode::On);
        let r = b.fadd("1.0", "2.0");
        assert_eq!(r, "%r1");
        assert_eq!(b.to_ir(), "entry.0:\n  %r1 = fadd contract double 1.0, 2.0");
    }

    #[test]
    fn fadd_emits_reassoc_when_fast_math_without_contract() {
        let mut b = fresh_with(true, FpContractMode::Off);
        let r = b.fadd("1.0", "2.0");
        assert_eq!(r, "%r1");
        assert_eq!(b.to_ir(), "entry.0:\n  %r1 = fadd reassoc double 1.0, 2.0");
    }

    #[test]
    fn fadd_emits_reassoc_and_contract_when_both_enabled() {
        let mut b = fresh_with(true, FpContractMode::Fast);
        let r = b.fadd("1.0", "2.0");
        assert_eq!(r, "%r1");
        assert_eq!(
            b.to_ir(),
            "entry.0:\n  %r1 = fadd reassoc contract double 1.0, 2.0"
        );
    }

    #[test]
    fn fp_flags_do_not_bleed_between_parallel_blocks() {
        let strict = thread::spawn(|| {
            let mut b = fresh_with(false, FpContractMode::Off);
            b.fmul("1.0", "2.0");
            b.to_ir()
        });
        let relaxed = thread::spawn(|| {
            let mut b = fresh_with(true, FpContractMode::On);
            b.fmul("1.0", "2.0");
            b.to_ir()
        });
        assert_eq!(
            strict.join().unwrap(),
            "entry.0:\n  %r1 = fmul double 1.0, 2.0"
        );
        assert_eq!(
            relaxed.join().unwrap(),
            "entry.0:\n  %r1 = fmul reassoc contract double 1.0, 2.0"
        );
    }

    #[test]
    fn call_with_args() {
        let mut b = fresh();
        let r = b.call(DOUBLE, "js_nanbox_string", &[(I64, "%handle")]);
        assert_eq!(r, "%r1");
        assert!(b
            .to_ir()
            .contains("call double @js_nanbox_string(i64 %handle)"));
    }

    #[test]
    fn terminator_blocks_further_emits() {
        let mut b = fresh();
        b.ret(DOUBLE, "0.0");
        // This would silently drop; we don't want extra lines after ret.
        let _ = b.fadd("1.0", "2.0");
        let ir = b.to_ir();
        assert!(ir.contains("ret double 0.0"));
        assert!(!ir.contains("fadd"));
    }

    #[test]
    fn regs_are_function_unique_not_block_unique() {
        let counter = Rc::new(RegCounter::new());
        let mut b1 = LlBlock::new("a", counter.clone());
        let mut b2 = LlBlock::new("b", counter);
        let r1 = b1.fadd("1.0", "2.0");
        let r2 = b2.fadd("3.0", "4.0");
        assert_eq!(r1, "%r1");
        assert_eq!(r2, "%r2");
    }
}
