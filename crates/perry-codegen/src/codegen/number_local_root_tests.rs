//! Charter step 5L, P6: a Number local never holds a pointer (step5 DESIGN
//! §3.4, §4.4), so it owns no root slot in any body kind, while a local that
//! can hold a pointer keeps its slot. The value guard that admits a raw double
//! into Number arithmetic ignores the sign bit, so the mirror of the tag band
//! (a negative NaN whose `fneg` image is a pointer tag) never passes it.

use crate::testing::root_slots::{function_slice, value_slot_binds};
use crate::testing::NativeRootsPin;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, Param, Stmt};

const SHORT_STRING_TAG_I64: &str = "9221401712017801216";
const MAGNITUDE_MASK_I64: &str = "9223372036854775807";

fn param(id: u32, ty: Type) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn function(id: u32, name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.into(),
        type_params: vec![],
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn ir_of(module: &Module) -> String {
    String::from_utf8(
        compile_module(
            module,
            CompileOptions {
                emit_ir_only: true,
                is_entry_module: false,
                ..CompileOptions::default()
            },
        )
        .expect("module compiles"),
    )
    .unwrap()
}

fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// `probe(a, b, c) { return a + b + c }` over erased operands: every leaf is
/// tested before the raw `fadd` arm. The test masks the sign bit and compares
/// the magnitude unsigned; the old signed compare admitted `0xFFFD_...`.
#[test]
fn number_guard_rejects_the_mirror_of_the_tag_band() {
    let mut m = Module::new("p6_guard.ts");
    m.functions.push(function(
        1,
        "probe",
        vec![
            param(1, Type::Any),
            param(2, Type::Any),
            param(3, Type::Any),
        ],
        vec![Stmt::Return(Some(bin(
            BinaryOp::Add,
            bin(BinaryOp::Add, Expr::LocalGet(1), Expr::LocalGet(2)),
            Expr::LocalGet(3),
        )))],
    ));
    let ir = ir_of(&m);
    let masked: Vec<&str> = ir
        .lines()
        .filter(|l| l.contains(" = and i64 ") && l.ends_with(&format!(", {MAGNITUDE_MASK_I64}")))
        .collect();
    assert!(
        !masked.is_empty(),
        "the guard must mask the sign bit:\n{ir}"
    );
    for line in &masked {
        let reg = line.trim().split(' ').next().unwrap();
        assert!(
            ir.lines().any(|l| l.contains("icmp ult i64 ")
                && l.contains(&format!("{reg}, {SHORT_STRING_TAG_I64}"))),
            "the masked magnitude {reg} must be compared unsigned below the band:\n{ir}"
        );
    }
    assert!(
        !ir.lines()
            .any(|l| l.contains("icmp slt i64 ")
                && l.ends_with(&format!(", {SHORT_STRING_TAG_I64}"))),
        "no signed tag-range guard may remain on the add path:\n{ir}"
    );
}

/// The admitted set is closed under the sign-only operations the fast arms
/// emit (`fneg`, `fabs`, `copysign`) and under NaN quieting, and contains both
/// default NaNs. A pointer tag's mirror is outside it.
#[test]
fn admitted_doubles_stay_outside_the_tag_band_under_sign_flips() {
    let band_start = crate::nanbox::SHORT_STRING_TAG;
    let admitted = |bits: u64| (bits & (i64::MAX as u64)) < band_start;
    let in_band = |bits: u64| bits >= band_start && bits <= u64::MAX >> 1;
    let samples = [
        0x7FF8_0000_0000_0000u64,
        0xFFF8_0000_0000_0000,
        0x7FF0_0000_0000_0001,
        0xFFF8_FFFF_FFFF_FFFF,
        0x7FF0_0000_0000_0000,
        (-0.0f64).to_bits(),
        1.5f64.to_bits(),
    ];
    for bits in samples {
        assert!(admitted(bits), "{bits:#x} is a genuine double");
        for image in [bits ^ (1 << 63), bits & (i64::MAX as u64), bits | (1 << 63)] {
            assert!(
                admitted(image) && !in_band(image),
                "{bits:#x} -> {image:#x}"
            );
        }
        if f64::from_bits(bits).is_nan() {
            assert!(admitted(bits | (1 << 51)), "quieting {bits:#x}");
        }
    }
    for tag in [
        crate::nanbox::POINTER_TAG,
        crate::nanbox::STRING_TAG,
        crate::nanbox::SHORT_STRING_TAG,
        crate::nanbox::BIGINT_TAG,
        crate::nanbox::INT32_TAG,
    ] {
        assert!(!admitted(tag | 0x1234), "{tag:#x} must be rejected");
        assert!(
            !admitted((tag | 0x1234) | (1 << 63)),
            "mirror of {tag:#x} must be rejected"
        );
    }
}

fn call0(callee: u32) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::LocalGet(callee)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    }
}

/// `probe(g) { return (h) => { let flags = 0; let any = 0;
///   flags = flags | h(); any = h(); return [flags, any] } }`
///
/// The tsc shape that fires (`flags |= f(x)` in 32 closures of the bundle):
/// the call result may be anything, so the pointer-local analysis reserves a
/// slot, but `|` yields a Number whatever its operands were (a BigInt operand
/// throws), so `flags` is a Number local and owns no slot. `any` holds the
/// call result itself and keeps its slot.
fn closure_module() -> Module {
    let mut m = Module::new("p6_closure.ts");
    let closure = Expr::Closure {
        func_id: 30,
        params: vec![param(20, Type::Any)],
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: 21,
                name: "flags".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::Let {
                id: 22,
                name: "any".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::Expr(Expr::LocalSet(
                21,
                Box::new(bin(BinaryOp::BitOr, Expr::LocalGet(21), call0(20))),
            )),
            Stmt::Expr(Expr::LocalSet(22, Box::new(call0(20)))),
            Stmt::Return(Some(Expr::Array(vec![
                Expr::LocalGet(21),
                Expr::LocalGet(22),
            ]))),
        ],
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    m.functions.push(function(
        1,
        "probe",
        vec![param(1, Type::Any)],
        vec![Stmt::Return(Some(closure))],
    ));
    m
}

/// The two stores differ only in what the local can hold: the Number local's
/// store is followed by no bind and no root shading, the pointer-capable
/// local's store still binds its slot.
#[test]
fn closure_number_local_owns_no_root_slot() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_of(&closure_module());
    let body = function_slice(&ir, "perry_closure_p6_closure_ts__30");
    assert!(
        value_slot_binds(body) >= 1,
        "the pointer-capable local must stay rooted (a module with binds):\n{body}"
    );
    // Exactly two value slots are bound: the parameter `h` and `any`. `flags`
    // (a raw integer here) had a reserved slot before P6, bound at its stores.
    let value_slots: Vec<String> = crate::testing::root_slots::bound_slots(body)
        .into_iter()
        .filter(|(_, (kind, _))| *kind == crate::testing::root_slots::SlotKind::Value)
        .map(|(slot, _)| slot)
        .collect();
    assert_eq!(
        value_slots.len(),
        2,
        "only `h` and `any` may own a bound root slot, got {value_slots:?}:\n{body}"
    );
    let any_slot = body
        .lines()
        .find_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("store double %")?;
            let (value, slot) = rest.split_once(", ptr ")?;
            body.lines()
                .any(|d| d.contains(&format!("%{value} = call double @js_closure_call0")))
                .then(|| slot.to_string())
        })
        .unwrap_or_else(|| panic!("`any = h()` must be stored:\n{body}"));
    assert!(
        value_slots.contains(&any_slot),
        "the pointer-capable `any` ({any_slot}) must stay rooted:\n{body}"
    );
}
