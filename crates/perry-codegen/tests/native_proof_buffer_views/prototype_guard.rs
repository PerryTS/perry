//! The Buffer numeric read fold is gated by Buffer.prototype's own shadow bit,
//! the word the runtime's Buffer name dispatch reads: every fold tests it and
//! a set bit takes the ordinary dispatch call. No syntactic scan of the
//! program decides it (`Buffer.prototype[k] = f`, `eval`, `new Function` all
//! set the bit at run time). An own property on the receiver is the operand
//! proof's business: the binding is no longer sealed, so nothing folds.

use super::*;

/// The shadow-bit compare: `icmp uge i32 %word, UNSHADOWED_WORD_END`
/// (`runtime_abi::LAYOUT_ORDINARY_END`).
const WRITTEN: &str = "-1879048192";

fn own_set(buffer_id: u32, method: &str) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(local(buffer_id)),
        property: method.to_string(),
        value: Box::new(int(1)),
        byte_offset: 0,
        byte_end: 0,
    })
}

#[test]
fn buffer_read_fold_tests_the_buffer_prototype_bit() {
    let ir = compile_ir(
        "buffer_read_prototype_guard.ts",
        vec![
            buffer_let(1, "buf", int(8)),
            Stmt::Expr(buffer_read(1, "readInt32BE", int(0))),
            Stmt::Return(Some(int(0))),
        ],
    );
    assert!(
        ir.contains("llvm.bswap.i32") && ir.contains("bytes.numeric.load"),
        "the read still folds:\n{ir}"
    );
    assert!(ir.contains(WRITTEN), "the fold tests the shadow bit:\n{ir}");
    assert_eq!(
        ir.matches("call double @js_native_call_method(").count(),
        1,
        "a written Buffer.prototype takes ONE ordinary dispatch call:\n{ir}"
    );
}

#[test]
fn a_reader_named_write_elsewhere_does_not_deopt_the_fold() {
    // Was the #6405 module scan: any `x.readInt32BE = …` in the module
    // disabled every fold. The bit decides now; `other` is not the receiver.
    let ir = compile_ir(
        "buffer_read_unrelated_shadow.ts",
        vec![
            buffer_let(1, "buf", int(8)),
            buffer_let(3, "other", int(8)),
            own_set(3, "readInt32BE"),
            Stmt::Expr(buffer_read(1, "readInt32BE", int(0))),
            Stmt::Return(Some(int(0))),
        ],
    );
    assert!(
        ir.contains("llvm.bswap.i32") && ir.contains(WRITTEN),
        "the read on `buf` folds under the guard:\n{ir}"
    );
}

#[test]
fn an_own_reader_on_the_receiver_never_folds() {
    // `buf.readInt32BE = f; buf.readInt32BE(0)`: the write exposes the
    // binding, so the operand proof refuses and the call dispatches.
    let ir = compile_ir(
        "buffer_read_own_shadow.ts",
        vec![
            buffer_let(1, "buf", int(8)),
            own_set(1, "readInt32BE"),
            Stmt::Expr(buffer_read(1, "readInt32BE", int(0))),
            Stmt::Return(Some(int(0))),
        ],
    );
    assert!(
        !ir.contains("llvm.bswap.i32") && !ir.contains("bytes.numeric.load"),
        "an own property shadows the prototype reader:\n{ir}"
    );
}
