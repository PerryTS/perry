//! `Buffer.prototype.equals` folds at proven sites only: a site the
//! whole-program facts prove (NativeMember) on two sealed constant-length byte
//! views is the decided length compare and one `memcmp`, with no test and no
//! method call. Any other site is the ordinary method call, which honours a
//! patched prototype and Node's argument checks.

use super::*;

const MEMCMP: &str = "call i32 @memcmp";

fn equals_at(site: u32, left_id: u32, right_id: u32) -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::PropertyGet {
            byte_end: 0,
            byte_offset: site,
            object: Box::new(local(left_id)),
            property: "equals".to_string(),
        }),
        args: vec![local(right_id)],
        type_args: Vec::new(),
        byte_offset: site,
    }
}

fn proven_equals(left_id: u32, right_id: u32) -> Expr {
    equals_at(PROVEN_SITE, left_id, right_id)
}

fn method_calls(ir: &str) -> usize {
    ir.matches("call double @js_method_call_slow").count()
        + ir.matches("call double @js_native_call_method").count()
}

fn pair_ir(name: &str, left: i64, right: i64) -> String {
    proven_ir(
        name,
        vec![
            buffer_let(1, "left", int(left)),
            buffer_let(2, "right", int(right)),
            Stmt::Return(Some(proven_equals(1, 2))),
        ],
    )
}

#[test]
fn proven_equals_is_one_memcmp_with_no_test_and_no_call() {
    for n in [16, 32, 64, 4096] {
        let ir = pair_ir("buffer_equals_proven.ts", n, n);
        assert!(
            ir.contains(&format!("{MEMCMP}(ptr %")) && ir.contains(&format!("i64 {n})")),
            "a proven {n}-byte equals is a constant-size memcmp:\n{ir}"
        );
        assert_eq!(
            method_calls(&ir),
            0,
            "no method call at the proven site:\n{ir}"
        );
        assert!(
            !ir.contains("-1879048192"),
            "no prototype shadow-bit test at the proven site:\n{ir}"
        );
    }
}

#[test]
fn proven_equals_decides_lengths_and_identity() {
    for (name, body, tag) in [
        (
            "buffer_equals_unequal_len.ts",
            vec![
                buffer_let(1, "l", int(8)),
                buffer_let(2, "r", int(16)),
                Stmt::Return(Some(proven_equals(1, 2))),
            ],
            perry_codegen::nanbox::TAG_FALSE_I64,
        ),
        (
            "buffer_equals_empty.ts",
            vec![
                buffer_let(1, "l", int(0)),
                buffer_let(2, "r", int(0)),
                Stmt::Return(Some(proven_equals(1, 2))),
            ],
            perry_codegen::nanbox::TAG_TRUE_I64,
        ),
        (
            "buffer_equals_self.ts",
            vec![
                buffer_let(1, "l", int(32)),
                Stmt::Return(Some(proven_equals(1, 1))),
            ],
            perry_codegen::nanbox::TAG_TRUE_I64,
        ),
    ] {
        let ir = proven_ir(name, body);
        assert!(
            !ir.contains(MEMCMP),
            "{name}: decided without memcmp:\n{ir}"
        );
        assert_eq!(method_calls(&ir), 0, "{name}: no method call:\n{ir}");
        assert!(ir.contains(tag), "{name}: folds to its constant:\n{ir}");
    }
}

#[test]
fn proven_equals_takes_a_uint8array_argument() {
    let ir = proven_ir(
        "buffer_equals_u8.ts",
        vec![
            buffer_let(1, "l", int(16)),
            typed_array_let(
                2,
                "u8",
                "Uint8Array",
                perry_hir::TYPED_ARRAY_KIND_UINT8,
                int(16),
            ),
            Stmt::Return(Some(proven_equals(1, 2))),
        ],
    );
    assert!(ir.contains(MEMCMP), "a Uint8Array argument folds:\n{ir}");
    assert_eq!(method_calls(&ir), 0, "{ir}");
}

#[test]
fn a_uint8clampedarray_argument_is_the_method_call() {
    // Node throws a TypeError: the argument is not a Uint8Array.
    let ir = proven_ir(
        "buffer_equals_clamped.ts",
        vec![
            buffer_let(1, "l", int(16)),
            typed_array_let(
                2,
                "c",
                "Uint8ClampedArray",
                perry_hir::TYPED_ARRAY_KIND_UINT8_CLAMPED,
                int(16),
            ),
            Stmt::Return(Some(proven_equals(1, 2))),
        ],
    );
    assert!(
        !ir.contains(MEMCMP),
        "no fold for a clamped argument:\n{ir}"
    );
    assert!(method_calls(&ir) > 0, "the ordinary call throws:\n{ir}");
}

#[test]
fn an_unproven_equals_is_the_ordinary_method_call() {
    let ir = compile_ir_with_opts(
        "buffer_equals_unproven.ts",
        vec![
            buffer_let(1, "l", int(16)),
            buffer_let(2, "r", int(16)),
            Stmt::Return(Some(equals_at(0, 1, 2))),
        ],
        proven_opts(),
    );
    assert!(!ir.contains(MEMCMP), "no fold at an unproven site:\n{ir}");
    assert!(method_calls(&ir) > 0, "the ordinary method call:\n{ir}");
    assert!(
        !ir.contains("-1879048192"),
        "an unproven site carries no shadow-bit test:\n{ir}"
    );
}

#[test]
fn an_exposed_argument_is_the_method_call() {
    // `r.buffer` may be transferred (detached, length 0): r is not sealed.
    let ir = proven_ir(
        "buffer_equals_exposed.ts",
        vec![
            buffer_let(1, "l", int(16)),
            buffer_let(2, "r", int(16)),
            Stmt::Expr(Expr::PropertyGet {
                byte_end: 0,
                byte_offset: 0,
                object: Box::new(local(2)),
                property: "buffer".to_string(),
            }),
            Stmt::Return(Some(proven_equals(1, 2))),
        ],
    );
    assert!(!ir.contains(MEMCMP), "no fold for an exposed view:\n{ir}");
}
