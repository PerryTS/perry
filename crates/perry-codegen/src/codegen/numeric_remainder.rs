//! Checked raw-Number remainder for typed entries.
//!
//! Keep the integer proof in the caller's LLVM unit: after inlining,
//! constants and guarded scalar lanes can discharge its roundtrip tests.
//! Source annotations never authorize an integer conversion.

use crate::module::LlModule;
use crate::types::{DOUBLE, I1, I32};

pub(crate) const CHECKED_NUMBER_REMAINDER: &str = "perry_checked_number_remainder";

pub(crate) fn define_checked_number_remainder_if_used(module: &mut LlModule) {
    let referenced = module.diagnostic_functions().any(|f| {
        f.blocks().iter().any(|b| {
            b.insts().iter().any(|inst| match inst {
                crate::inst::LlInst::Call { callee, .. } => callee == CHECKED_NUMBER_REMAINDER,
                // Calls under a catch handler use the existing raw invoke
                // carrier. Match this exact compiler-owned symbol as well.
                crate::inst::LlInst::Raw(line) => line.contains("@perry_checked_number_remainder("),
                _ => false,
            })
        })
    });
    if !referenced {
        return;
    }
    let f = module.define_function(
        CHECKED_NUMBER_REMAINDER,
        DOUBLE,
        vec![(DOUBLE, "%left".into()), (DOUBLE, "%right".into())],
    );
    // Identical operation-level code in every source module. Weak ODR also
    // permits codegen-unit partitioning without duplicate strong symbols.
    f.linkage = "linkonce_odr".into();
    f.pre_statepoint_inline = true;
    f.create_block("entry");
    let integer = f.create_block("mod.i32.integer").label.clone();
    let fallback = f.create_block("mod.f64.fallback").label.clone();
    let (li, ri) = {
        let blk = f.block_mut(0).unwrap();
        // fptosi outside i32 produces poison, not immediate UB. Freeze makes
        // that a stable arbitrary integer. Its exact f64 roundtrip CANNOT
        // equal an out-of-range/fractional/NaN/infinite original operand.
        // Only these frozen values, never the poison conversions, are used.
        // This is the same admission proof used by expr/typed_element.rs.
        let converted = blk.fptosi(DOUBLE, "%left", I32);
        let li = blk.freeze(I32, &converted);
        let converted = blk.fptosi(DOUBLE, "%right", I32);
        let ri = blk.freeze(I32, &converted);
        let l_back = blk.sitofp(I32, &li, DOUBLE);
        let r_back = blk.sitofp(I32, &ri, DOUBLE);
        let l_exact = blk.fcmp("oeq", &l_back, "%left");
        let r_exact = blk.fcmp("oeq", &r_back, "%right");
        let nonzero = blk.icmp_ne(I32, &ri, "0");
        let exact = blk.and(I1, &l_exact, &r_exact);
        let valid = blk.and(I1, &exact, &nonzero);
        blk.cond_br(&valid, &integer, &fallback);
        (li, ri)
    };
    {
        let blk = f.block_mut(1).unwrap();
        // LLVM srem(INT_MIN,-1) is poison. Replacing -1 by +1 preserves
        // every integer remainder; the dividend's sign is restored below.
        let minus_one = blk.icmp_eq(I32, &ri, "-1");
        let divisor = blk.select(I1, &minus_one, I32, "1", &ri);
        let rem = blk.srem(I32, &li, &divisor);
        let result = blk.sitofp(I32, &rem, DOUBLE);
        // A nonzero signed remainder already has the dividend's sign. Copy
        // that sign for ALL results, including -0 % n and negative multiples,
        // without a zero-result branch or a floating-point comparison.
        let result = blk.call_gc_leaf(
            DOUBLE,
            "llvm.copysign.f64",
            &[(DOUBLE, &result), (DOUBLE, "%left")],
        );
        blk.ret(DOUBLE, &result);
    }
    {
        let blk = f.block_mut(2).unwrap();
        // Exact i53 and all IEEE-754 exceptional/fractional cases retain
        // the existing runtime implementation and evaluation order.
        let result = blk.call_gc_leaf(
            DOUBLE,
            "js_number_mod",
            &[(DOUBLE, "%left"), (DOUBLE, "%right")],
        );
        blk.ret(DOUBLE, &result);
    }
}
