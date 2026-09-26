//! User-visible ECMAScript ToInt32 with a guarded hardware fast path (#10897).
//!
//! `x | 0`, `~x`, the operands of `&` `|` `^` `<<` `>>` `>>>`, integer typed
//! array stores and the entry conversion into a canonical i32 slot are all
//! spec ToInt32, and must be exact for every Number. The exact lowering,
//! [`LlBlock::toint32_wrap_branchless`], is a ~25-op exponent/mantissa tower,
//! because LLVM `fptosi` is poison outside its destination range and a bare
//! conversion would print garbage for `(1e20) | 0`. Running that tower on every
//! evaluation made `h = (h + o.a) | 0` cost +43 instructions/iteration over
//! `h = h + o.a` — more than the property read beside it.
//!
//! Every Number with `|v| < 2^63` is exact through `fptosi double -> i64`, and
//! the low 32 bits of that truncation ARE ToInt32 (truncate toward zero, then
//! reduce modulo 2^32). So test the magnitude once and run the tower only for
//! what the hardware conversion cannot take — NaN, ±Infinity and `|v| >= 2^63`
//! — on a cold arm. That is the shape V8 and JSC emit: `cvttsd2si` plus a
//! rarely-taken out-of-range fixup.
//!
//! A double the current block just widened from an i32 needs no conversion
//! at all: `h = (h + o.a) | 0` into a canonical i32 `h` produces
//! `sitofp(i32)` for the expression's value and immediately converts it
//! back for the slot, and LLVM does not fold the magnitude guard on a
//! `sitofp` result.
//!
//! [`LlBlock::toint32_wrap_branchless`]: crate::block::LlBlock::toint32_wrap_branchless

use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64};

/// 2^63, the first magnitude at which `fptosi double -> i64` is poison.
const FPTOSI_I64_LIMIT: &str = "0x43E0000000000000";

impl FnCtx<'_> {
    /// ECMAScript ToInt32 of a Number, exact for ALL inputs, lowered as a
    /// predicted diamond: `fptosi`+`trunc` when `|val| < 2^63`, the branchless
    /// tower otherwise. Returns an `i32` valid in the (new) current block.
    ///
    /// Opens blocks, so a caller building a phi around this must take its
    /// incoming label from the current block AFTER the call.
    pub(crate) fn toint32_wrap(&mut self, val: &str) -> String {
        // A double this block just widened from an i32 (`(h + o.a) | 0`
        // entering h's i32 slot) converts back to exactly that i32.
        if let Some(int) = self.block().widened_i32_source(val) {
            return int;
        }
        // `fjcvtzs` is the whole conversion in one instruction: nothing to
        // branch around.
        if crate::codegen::helpers::jscvt_enabled() {
            return self.block().toint32_wrap_branchless(val);
        }
        let magnitude = self.block().call(DOUBLE, "llvm.fabs.f64", &[(DOUBLE, val)]);
        // Ordered compare: NaN is false, so NaN joins ±Infinity on the exact arm.
        let in_range = self.block().fcmp("olt", &magnitude, FPTOSI_I64_LIMIT);
        let in_range = self
            .block()
            .call(I1, "llvm.expect.i1", &[(I1, &in_range), (I1, "true")]);
        let fast_idx = self.new_block("toint32.fast");
        let exact_idx = self.new_block("toint32.exact");
        let merge_idx = self.new_block("toint32.merge");
        let fast_label = self.block_label(fast_idx);
        let exact_label = self.block_label(exact_idx);
        let merge_label = self.block_label(merge_idx);
        self.block().cond_br(&in_range, &fast_label, &exact_label);

        self.current_block = fast_idx;
        let wide = self.block().fptosi(DOUBLE, val, I64);
        let fast = self.block().trunc(I64, &wide, I32);
        self.block().br(&merge_label);

        self.current_block = exact_idx;
        let exact = self.block().toint32_wrap_branchless(val);
        self.block().br(&merge_label);

        self.current_block = merge_idx;
        self.block()
            .phi(I32, &[(&fast, &fast_label), (&exact, &exact_label)])
    }
}
