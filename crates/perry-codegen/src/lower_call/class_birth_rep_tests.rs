//! Charter step 5, T1: the class birth rep is decided once, in codegen
//! (`typed_shape::class_birth_rep_in`), and every consumer takes it from
//! there: the module-init mint, the inline allocation's birth fill and the
//! class-field store precheck.

use super::typed_shape_bake_tests::{emit, loop_new_module};
use perry_hir::types::Type;
use perry_hir::Expr;

/// The last argument of the first call to `callee` in `ir`.
fn mint_rep(ir: &str, callee: &str) -> String {
    let at = ir
        .find(&format!("call i32 @{callee}("))
        .unwrap_or_else(|| panic!("no {callee} call:\n{ir}"));
    let line = ir[at..].lines().next().unwrap();
    let args = line.rsplit_once(')').unwrap().0;
    args.rsplit_once(", ").unwrap().1.to_string()
}

/// `class Early { a: number = this.b; b: number }`: the initializer of `a`
/// observes the instance before any field is written, so nothing is `F64`.
fn early_module() -> perry_hir::Module {
    let mut m = loop_new_module("Early", Type::Number, Expr::Integer(2));
    m.classes[0].fields[0].init = Some(Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::This),
        property: "b".to_string(),
        byte_offset: 0,
    });
    m
}

/// (a): the mint carries `F64` for exactly the `number` fields written before
/// any code can observe the instance (option (a), `birth_lanes`): constructor
/// prologue stores and literal initializers alike.
#[test]
fn the_birth_rep_is_f64_for_exactly_the_fields_written_before_any_observation() {
    // `class Pair { a: number; b: number }`, both prologue-assigned.
    let ir = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    assert_eq!(
        mint_rep(&ir, "js_object_layout_for_class_keys_static"),
        "i64 5"
    );
    // `class Link { a: number; b: string }`: only slot 0 is `number`.
    let ir = emit(&loop_new_module(
        "Link",
        Type::String,
        Expr::String("s".into()),
    ));
    assert_eq!(
        mint_rep(&ir, "js_object_layout_for_class_keys_static"),
        "i64 1"
    );
    // `class Late { a: number; b: number = 3 }`: the literal initializer is
    // `b`'s first write, before anything can read it.
    let mut m = loop_new_module("Late", Type::Number, Expr::Integer(2));
    m.classes[0].fields[1].init = Some(Expr::Number(3.0));
    let ir = emit(&m);
    assert_eq!(
        mint_rep(&ir, "js_object_layout_for_class_keys_static"),
        "i64 5"
    );
    // `class Str { a: number; b: number = "s" }`: a non-Number first write.
    let mut m = loop_new_module("Str", Type::Number, Expr::Integer(2));
    m.classes[0].fields[1].init = Some(Expr::String("s".into()));
    let ir = emit(&m);
    assert_eq!(
        mint_rep(&ir, "js_object_layout_for_class_keys_static"),
        "i64 1"
    );
    let ir = emit(&early_module());
    assert_eq!(
        mint_rep(&ir, "js_object_layout_for_class_keys_static"),
        "i64 0"
    );
}

/// (c) + (d): the shared initializer receives the exact rep the mint declared.
/// Its bounded two-bit lane walk selects +0.0 for F64 and undefined for Any.
#[test]
fn the_inline_allocation_fills_exactly_the_minted_f64_lanes() {
    let declared = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    let late = emit(&early_module());
    for (ir, expected) in [(&declared, "i64 5"), (&late, "i64 0")] {
        assert_eq!(
            mint_rep(ir, "js_object_layout_for_class_keys_static"),
            expected
        );
        let calls: Vec<_> = ir
            .lines()
            .filter(|l| l.contains("call preserve_mostcc i64 @perry_birth_class("))
            .collect();
        assert!(!calls.is_empty(), "the shared slow birth must be live");
        for call in calls {
            let args = call.split_once(')').unwrap().0;
            assert_eq!(args.rsplit_once(", ").unwrap().1, expected, "{call}");
        }
        assert!(ir.contains("%lane = and i64 %lanes, 3"));
        assert!(ir.contains("%f64 = icmp eq i64 %lane, 1"));
        assert!(ir.contains(&format!(
            "%default = select i1 %f64, i64 0, i64 {}",
            crate::nanbox::TAG_UNDEFINED_I64
        )));
        assert!(ir.contains("%remaining = lshr i64 %lanes, 2"));
        assert!(ir.contains("%slots_more = icmp ult i64 %j_next, %slots"));
        assert!(ir.contains("store i64 %default, ptr %typed_slot"));
    }
}

#[test]
fn a_store_to_a_reservation_can_observe_later_defaults_through_a_setter() {
    let mut m = loop_new_module("ObservedStore", Type::Number, Expr::Integer(2));
    m.classes[0].constructor.as_mut().unwrap().body.insert(
        0,
        perry_hir::Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "signal".into(),
            value: Box::new(Expr::Number(1.0)),
            byte_offset: 0,
            byte_end: 0,
        }),
    );
    let ir = emit(&m);
    assert_eq!(
        mint_rep(&ir, "js_object_layout_for_class_keys_static"),
        "i64 0"
    );
}

#[test]
fn numeric_reservations_keep_the_completed_contract_after_constructor_observations() {
    let mut m = loop_new_module("ReservedNumbers", Type::Number, Expr::Integer(2));
    for field in &mut m.classes[0].fields {
        field.origin = perry_hir::ClassFieldOrigin::ConstructorStore;
    }
    m.classes[0].constructor.as_mut().unwrap().body.insert(
        0,
        perry_hir::Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "signal".into(),
            value: Box::new(Expr::Number(1.0)),
            byte_offset: 0,
            byte_end: 0,
        }),
    );
    let ir = emit(&m);
    assert_eq!(
        mint_rep(&ir, "js_object_layout_for_class_keys_static"),
        "i64 5"
    );
    assert!(
        ir.lines().any(|line| {
            line.contains("@js_object_layout_for_class_keys_static(i64 0, i32 0,")
                && line.ends_with("i64 0)")
        }),
        "the reservation birth must stay keyless and Any"
    );
}

fn check_compact_birth(ir: &str, defaults: &str) {
    let mut checked = 0;
    for function in ir.split("\ndefine ").skip(1) {
        let defs: std::collections::HashMap<_, _> = function
            .lines()
            .filter_map(|line| line.trim().split_once(" = "))
            .collect();
        for tail in function.split("\nalloc.fast.").skip(1) {
            let label = format!("alloc.fast.{}", tail.split_once(':').unwrap().0);
            let branch = function
                .lines()
                .find(|line| line.contains(&format!("label %{label},")))
                .unwrap();
            let condition = branch
                .trim()
                .strip_prefix("br i1 ")
                .unwrap()
                .split(',')
                .next()
                .unwrap();
            let compare = defs[condition].strip_prefix("icmp ule i64 ").unwrap();
            let limit = compare.split_once(", ").unwrap().1;
            let address = defs[limit]
                .strip_prefix("load i64, ptr ")
                .expect("the fast birth must consume the live canonical bump bound")
                .split(',')
                .next()
                .unwrap();
            assert!(
                defs[address].starts_with("getelementptr i8, ptr ")
                    && defs[address].ends_with(", i64 16"),
                "permission must come from InlineArenaState.size"
            );
            let body = tail.split_once(':').unwrap().1;
            let body = body
                .lines()
                .take_while(|line| line.is_empty() || line.starts_with(' '))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!body.contains("call "), "compact fast birth must be leaf");
            assert!(
                body.contains("store <2 x i64> %"),
                "missing birth shape/header"
            );
            assert!(body.contains("store i64 0, ptr "), "missing null metadata");
            assert!(
                body.contains(defaults),
                "missing exact capacity defaults: {body}"
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "compact birth subject must be live");
    assert!(
        ir.contains("load volatile i8"),
        "the shared fallback must still read live birth color"
    );
}

/// The compact birth of a numeric (`numeric = true`) or an any-typed field:
/// its IR and the exact capacity-default store.
fn compact_birth_case(numeric: bool) -> (String, String) {
    let undefined = crate::nanbox::TAG_UNDEFINED_I64;
    let (ty, rhs, defaults) = if numeric {
        (
            Type::Number,
            Expr::Integer(2),
            "store <2 x i64> <i64 0, i64 0>".to_string(),
        )
    } else {
        (
            Type::String,
            Expr::String("s".into()),
            format!("store <2 x i64> <i64 0, i64 {undefined}>"),
        )
    };
    (emit(&loop_new_module("Compact", ty, rhs)), defaults)
}

/// Negative control: the birth no longer initializes its capacity slots.
fn without_capacity_initialization(ir: &str, defaults: &str) -> String {
    let broken = ir.replace(defaults, "; removed capacity initialization");
    assert_ne!(broken, ir, "negative control must alter a real birth store");
    broken
}

/// Negative control: the fast path's permission no longer comes from the
/// canonical bump bound.
fn without_canonical_bump_bound(ir: &str) -> String {
    let broken = ir.replace(", i64 16\n", ", i64 24\n");
    assert_ne!(
        broken, ir,
        "negative control must revoke the canonical bump-bound proof"
    );
    broken
}

#[test]
fn compact_birth_has_no_call_and_initializes_any_and_numeric_capacity_slots() {
    for numeric in [true, false] {
        let (ir, defaults) = compact_birth_case(numeric);
        check_compact_birth(&ir, &defaults);
    }
}

#[test]
#[should_panic(expected = "missing exact capacity defaults")]
fn compact_numeric_birth_without_capacity_initialization_is_refused() {
    let (ir, defaults) = compact_birth_case(true);
    check_compact_birth(&without_capacity_initialization(&ir, &defaults), &defaults);
}

#[test]
#[should_panic(expected = "missing exact capacity defaults")]
fn compact_any_birth_without_capacity_initialization_is_refused() {
    let (ir, defaults) = compact_birth_case(false);
    check_compact_birth(&without_capacity_initialization(&ir, &defaults), &defaults);
}

#[test]
#[should_panic(expected = "permission must come from InlineArenaState.size")]
fn compact_numeric_birth_without_the_canonical_bump_bound_is_refused() {
    let (ir, defaults) = compact_birth_case(true);
    check_compact_birth(&without_canonical_bump_bound(&ir), &defaults);
}

#[test]
#[should_panic(expected = "permission must come from InlineArenaState.size")]
fn compact_any_birth_without_the_canonical_bump_bound_is_refused() {
    let (ir, defaults) = compact_birth_case(false);
    check_compact_birth(&without_canonical_bump_bound(&ir), &defaults);
}
