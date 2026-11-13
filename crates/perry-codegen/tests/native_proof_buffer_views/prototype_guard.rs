//! The Buffer numeric read fold is the proven path only: a reader site the
//! whole-program facts prove (NativeMember: no write may reach
//! `Buffer.prototype`'s reader, by the program or by interpreted code) is the
//! native load with no test; any other site is the ordinary method call (no
//! fold, no shadow-bit test, no dispatch arm). An own property on the
//! receiver is the operand proof's business: the binding is no longer
//! sealed, so nothing folds.

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
fn a_reader_named_write_elsewhere_does_not_deopt_the_fold() {
    // Was the #6405 module scan: any `x.readInt32BE = …` in the module
    // disabled every fold. The proof decides now; `other` is not the receiver.
    let ir = compile_ir_with_opts(
        "buffer_read_unrelated_shadow.ts",
        vec![
            buffer_let(1, "buf", int(8)),
            buffer_let(3, "other", int(8)),
            own_set(3, "readInt32BE"),
            Stmt::Expr(proven_buffer_read(1, "readInt32BE", int(0))),
            Stmt::Return(Some(int(0))),
        ],
        proven_opts(),
    );
    assert!(
        ir.contains("call i32 @llvm.bswap.i32(") && !ir.contains(WRITTEN),
        "the proven read on the sealed `buf` is the native load with no test:\n{ir}"
    );
    assert_eq!(
        ir.matches("call double @js_method_call_slow(").count()
            + ir.matches("call double @js_native_call_method(").count(),
        0,
        "the proven read on `buf` is no method call:\n{ir}"
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
        !ir.contains("call i32 @llvm.bswap.i32(") && !ir.contains("bytes.numeric.load"),
        "an own property shadows the prototype reader:\n{ir}"
    );
}

/// The callee read's offset of the reader site under test.
const SITE: u32 = 41;

/// `const buf = Buffer.alloc(8); buf.readInt32BE(0)` with the callee read at
/// `SITE`, compiled with `proven` as the module's NativeMember sites. The
/// same HIR both ways: the only input that differs is the proof.
fn reader_site_ir(name: &str, proven: &[u32]) -> String {
    let elision = perry_codegen::ElisionFacts {
        native_member_calls: proven.iter().copied().collect(),
        ..Default::default()
    };
    let opts = CompileOptions {
        program_facts: perry_codegen::ProgramFactsView {
            elision: std::sync::Arc::new(elision),
            ..Default::default()
        },
        ..empty_opts()
    };
    compile_ir_with_opts(
        name,
        vec![
            buffer_let(1, "buf", int(8)),
            Stmt::Expr(Expr::Call {
                byte_end: 0,
                callee: Box::new(Expr::PropertyGet {
                    byte_end: 0,
                    byte_offset: SITE,
                    object: Box::new(local(1)),
                    property: "readInt32BE".to_string(),
                }),
                args: vec![int(0)],
                type_args: Vec::new(),
                byte_offset: SITE,
            }),
            Stmt::Return(Some(int(0))),
        ],
        opts,
    )
}

#[test]
fn a_proven_reader_site_is_the_native_load_alone() {
    // wpfacts NativeMember: no write may reach Buffer.prototype's reader.
    let ir = reader_site_ir("buffer_read_proven_site.ts", &[SITE]);
    assert!(
        ir.contains("call i32 @llvm.bswap.i32("),
        "the proven read folds:\n{ir}"
    );
    assert!(!ir.contains(WRITTEN), "a proven site tests no bit:\n{ir}");
    assert_eq!(
        ir.matches("call double @js_native_call_method(").count(),
        0,
        "a proven site has no dispatch arm:\n{ir}"
    );
}

#[test]
fn an_unproven_reader_site_is_the_ordinary_method_call() {
    let ir = reader_site_ir("buffer_read_unproven_site.ts", &[]);
    assert!(
        !ir.contains("bytes.numeric.load") && !ir.contains(WRITTEN),
        "an unproven read neither folds nor tests a bit:\n{ir}"
    );
    assert_eq!(
        ir.matches("call double @js_method_call_slow(").count(),
        1,
        "an unproven read is ONE ordinary method call:\n{ir}"
    );
}
