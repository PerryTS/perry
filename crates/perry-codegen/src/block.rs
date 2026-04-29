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
use std::rc::Rc;

use crate::types::LlvmType;

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
}

impl LlBlock {
    pub fn new(label: impl Into<String>, counter: Rc<RegCounter>) -> Self {
        Self {
            label: label.into(),
            instructions: Vec::new(),
            terminated: false,
            counter,
        }
    }

    pub fn is_terminated(&self) -> bool {
        self.terminated
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
    // We emit `reassoc contract` fast-math flags on every float op. These
    // are the two LLVM FMFs that unlock the optimizations we actually want
    // on tight numeric loops:
    //
    //   - `reassoc`: lets LLVM reorder `(a + b) + c → a + (b + c)`, which
    //     is what the loop-vectorizer needs to break a serial accumulator
    //     chain into 4 parallel accumulators. Without it, `sum += 1` in a
    //     100M-iter loop runs at the 3-cycle fadd latency (~100ms); with
    //     it, LLVM unrolls 8x + vectorizes 2-wide + splits into 4 parallel
    //     vector accumulators → ~12ms, beating Node (~60ms) and Bun by 4x.
    //   - `contract`: allow fused multiply-add (FMA). A single FMA is 2
    //     ops in 1 instruction (and 1 rounding step), which speeds up any
    //     `x * y + z` pattern, which is common in matrix and vector math.
    //
    // We deliberately DON'T emit the full `fast` flag set (`nnan ninf nsz
    // arcp contract afn reassoc`). `nnan` and `ninf` in particular are
    // UB-style flags — they tell LLVM to assume no NaN or Inf inputs,
    // which is catastrophic for Perry: NaN-boxing uses NaN bit patterns
    // for EVERY non-number value (strings, objects, null, undefined,
    // booleans). Passing `-ffast-math` to clang was tried briefly at
    // v0.2-era commit 083ce16 and reverted two days later in b5a8c83f
    // because `-ffinite-math-only` (implied by `-ffast-math`) made LLVM
    // replace TAG_NULL / TAG_UNDEFINED constants with 0.0 at codegen
    // time. The clang step now passes `-fno-math-errno` only — every
    // fast-math effect in Perry comes from the per-instruction FMFs
    // emitted here.
    //
    // For reference, Rust's nightly `#![feature(float_algebraic)]`
    // enables `reassoc + contract + nsz + arcp + afn` — a broader set
    // than Perry's two flags. So Perry's default is strictly more
    // conservative than Rust's nightly opt-in per-operation API.

    pub fn fadd(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fadd reassoc contract double {}, {}", r, a, b));
        r
    }

    pub fn fsub(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fsub reassoc contract double {}, {}", r, a, b));
        r
    }

    pub fn fmul(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fmul reassoc contract double {}, {}", r, a, b));
        r
    }

    pub fn fdiv(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fdiv reassoc contract double {}, {}", r, a, b));
        r
    }

    pub fn frem(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = frem reassoc contract double {}, {}", r, a, b));
        r
    }

    pub fn fneg(&mut self, a: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fneg reassoc contract double {}", r, a));
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

    /// Store with `volatile` — prevents optimizer from eliminating or
    /// reordering. Used for module globals.
    pub fn store_volatile(&mut self, ty: LlvmType, val: &str, ptr: &str) {
        self.emit(format!("store volatile {} {}, ptr {}", ty, val, ptr));
    }

    // -------- Conversions / bitcasts --------

    pub fn bitcast_i64_to_double(&mut self, val: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = bitcast i64 {} to double", r, val));
        r
    }

    pub fn bitcast_double_to_i64(&mut self, val: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = bitcast double {} to i64", r, val));
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

    pub fn fptosi(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = fptosi {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    /// ECMAScript ToInt32: `fptosi` with a NaN/Infinity guard.
    /// JS ToInt32: NaN and ±Infinity produce 0 (per spec), normal values
    /// go through `fptosi(f64→i64) + trunc(i64→i32)`.
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

    /// Signed integer division.  Emitted by the `(int / int) | 0` fast
    /// path — avoids `scvtf → fdiv → fcvtzs` and lets LLVM replace
    /// constant divisors with `smulh + asr`.
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
        let r = self.reg();
        let arg_str = format_args(args);
        self.emit(format!(
            "{} = call {} @{}({})",
            r, ret_ty, func_name, arg_str
        ));
        r
    }

    pub fn call_void(&mut self, func_name: &str, args: &[(LlvmType, &str)]) {
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

    fn fresh() -> LlBlock {
        LlBlock::new("entry.0", Rc::new(RegCounter::new()))
    }

    #[test]
    fn fadd_emits_expected_ir() {
        let mut b = fresh();
        let r = b.fadd("1.0", "2.0");
        assert_eq!(r, "%r1");
        assert_eq!(
            b.to_ir(),
            "entry.0:\n  %r1 = fadd reassoc contract double 1.0, 2.0"
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
