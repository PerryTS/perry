use super::*;
use perry_hir::{types::Type, CatchClause, Function, Module, Param};

fn get(object: Expr, key: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(object),
        property: key.into(),
        byte_offset: 0,
    }
}

fn compare(op: CompareOp, left: Expr, right: Expr) -> Expr {
    Expr::Compare {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn ir(body: Vec<Stmt>, in_loop: bool) -> String {
    let param = |id| Param {
        id,
        name: format!("p{id}"),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let protected = Stmt::Try {
        body,
        catch: Some(CatchClause {
            param: None,
            body: Vec::new(),
        }),
        finally: None,
    };
    let body = if in_loop {
        vec![Stmt::While {
            condition: Expr::Bool(true),
            body: vec![protected, Stmt::Break],
        }]
    } else {
        vec![protected]
    };
    let mut module = Module::new("bounded_numeric");
    module.functions.push(Function {
        id: 1,
        name: "probe".into(),
        type_params: Vec::new(),
        params: vec![param(1), param(2)],
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let mut opts = crate::temp_root_coverage::entry_opts();
    opts.is_entry_module = false;
    String::from_utf8(crate::compile_module(&module, opts).expect("bounded expression compiles"))
        .unwrap()
}

fn function(ir: &str) -> &str {
    let start = ir
        .find("define double @perry_fn_bounded_numeric__probe")
        .expect("probe body");
    let body = &ir[start..];
    &body[..body.find("\n}").expect("end of probe")]
}

#[test]
fn numeric_expression_all_relations_and_orientations_use_exact_r_guard() {
    for (op, pred) in [
        (CompareOp::Lt, "olt"),
        (CompareOp::Le, "ole"),
        (CompareOp::Gt, "ogt"),
        (CompareOp::Ge, "oge"),
    ] {
        for read_left in [true, false] {
            let field = get(Expr::LocalGet(1), "value");
            let expr = if read_left {
                compare(op, field, Expr::Integer(166))
            } else {
                compare(op, Expr::LocalGet(2), field)
            };
            let output = ir(vec![Stmt::Expr(expr)], true);
            let body = function(&output);
            assert!(body.contains("rexpr.fast"), "{op:?} left={read_left}");
            assert!(body.contains(&format!("fcmp {pred} double")));
            let prime = body
                .lines()
                .find(|l| l.contains("@js_region_loop_prime("))
                .expect("fresh runtime supplier");
            assert!(
                prime.contains("i32 0, i32 0, i32 1)"),
                "stored=0, boxed=0, R=1: {prime}"
            );
            assert_eq!(
                body.matches("landingpad ").count(),
                1,
                "single handler tree"
            );
            assert!(
                !body.contains("rloop.version.split"),
                "whole Try body still refused"
            );
        }
    }
}

#[test]
fn numeric_expression_saves_left_once_before_guard_and_keeps_eh_target() {
    crate::temp_root_coverage::under_both_lowerings(|_| saves_left_once());
}

fn saves_left_once() {
    let left = get(get(Expr::LocalGet(2), "data"), "length");
    let output = ir(
        vec![Stmt::Expr(compare(
            CompareOp::Lt,
            left,
            get(Expr::LocalGet(1), "value"),
        ))],
        true,
    );
    let body = function(&output);
    let fast = body.find("rexpr.fast").expect("expression region");
    let prime = body.find("@js_region_loop_prime(").unwrap();
    assert!(
        body[..fast].contains("invoke "),
        "left getter retains invoke before guard"
    );
    assert!(
        body.contains("9223372036854775807"),
        "saved value gets strict Number entry check"
    );
    // The result of the left GetValue is the final property-result phi
    // before the guard word load. Assert that exact value is a scoped root,
    // and that F rereads that slot below the guard rather than retaining SSA.
    let fast_block = body
        .split("rexpr.fast.")
        .find(|s| s.lines().next().is_some_and(|l| l.ends_with(':')))
        .unwrap();
    let fast_body = fast_block.split("\n\n").next().unwrap();
    let guard_load = body
        .lines()
        .find(|l| l.contains("load atomic i64") && l.contains("_region "))
        .unwrap();
    let before_guard = &body[..body.find(guard_load).unwrap()];
    let slot = before_guard
        .lines()
        .filter_map(|line| {
            line.contains(" = phi double ")
                .then(|| line.trim().split(" =").next().unwrap())
        })
        .find_map(|value| {
            crate::testing::temp_slots::temp_root_slot_holding(body, value)
                .filter(|slot| fast_body.contains(&format!("ptr {slot}")))
        })
        .expect("saved left GetValue has a managed temporary root reread in F");
    assert!(
        fast_body.contains(&format!("ptr {slot}")),
        "F rereads saved left root"
    );
    assert!(
        fast_body.contains("getelementptr double"),
        "exact R load is reached in F"
    );
    assert!(
        !fast_body.contains("@js_object_get_field"),
        "F read is bare"
    );
    assert!(
        body[..prime].contains(&format!("ptr {slot}")),
        "root published before prime"
    );
    let unwind_targets: HashSet<_> = body
        .lines()
        .filter_map(|l| l.split("unwind label %").nth(1))
        .collect();
    assert_eq!(
        unwind_targets.len(),
        1,
        "left reads, fallback getters and coercions retain same active pad"
    );
    assert_eq!(
        body.lines()
            .filter(|l| l.contains("@js_object_get_field_ic_slow("))
            .count(),
        3,
        "data, length and G value have exactly one lowering each; F value is bare"
    );
}

#[test]
fn numeric_expression_js_invoke_before_read_discards_f() {
    TEST_REENTER_BEFORE_READ.with(|flag| flag.set(true));
    let output = ir(
        vec![Stmt::Expr(compare(
            CompareOp::Lt,
            get(Expr::LocalGet(1), "value"),
            Expr::Integer(166),
        ))],
        true,
    );
    TEST_REENTER_BEFORE_READ.with(|flag| flag.set(false));
    let body = function(&output);
    assert!(
        body.lines()
            .any(|l| l.contains("invoke double @js_rel_lt(double 0.0, double 0.0)")),
        "negative control is a JS-capable invoke in the active handler"
    );
    let guard_load = body
        .lines()
        .find(|l| l.contains("load atomic i64") && l.contains("_region "))
        .unwrap();
    let decision = &body[body.find(guard_load).unwrap()..];
    let branch = decision
        .lines()
        .find(|l| l.trim_start().starts_with("br label %"))
        .unwrap();
    assert!(
        branch.contains("%rexpr.slow."),
        "unverified F cannot execute: {branch}"
    );
}

#[test]
fn numeric_expression_boxed_receiver_is_refused() {
    let closure = Expr::Closure {
        func_id: 2,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Undefined)))],
        captures: vec![1],
        mutable_captures: vec![1],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    let output = ir(
        vec![
            Stmt::Expr(closure),
            Stmt::Expr(compare(
                CompareOp::Ge,
                get(Expr::LocalGet(1), "value"),
                Expr::Integer(166),
            )),
        ],
        true,
    );
    assert!(!function(&output).contains("rexpr.fast"));
}

#[test]
fn numeric_expression_refuses_equality_dynamic_receiver_and_straight_line() {
    for expr in [
        compare(
            CompareOp::Eq,
            get(Expr::LocalGet(1), "value"),
            Expr::Integer(1),
        ),
        compare(
            CompareOp::Lt,
            get(get(Expr::LocalGet(1), "nested"), "value"),
            Expr::Integer(1),
        ),
        compare(
            CompareOp::Lt,
            get(Expr::LocalGet(1), "value"),
            get(get(Expr::LocalGet(2), "nested"), "other"),
        ),
    ] {
        assert!(!function(&ir(vec![Stmt::Expr(expr)], true)).contains("rexpr.fast"));
    }
    let expr = compare(
        CompareOp::Ge,
        get(Expr::LocalGet(1), "value"),
        Expr::Integer(166),
    );
    assert!(!function(&ir(vec![Stmt::Expr(expr)], false)).contains("rexpr.fast"));
    let labeled = Stmt::Labeled {
        label: "outer".into(),
        body: Box::new(Stmt::Expr(compare(
            CompareOp::Ge,
            get(Expr::LocalGet(1), "value"),
            Expr::Integer(166),
        ))),
    };
    assert!(!function(&ir(vec![labeled], true)).contains("rexpr.fast"));
}

#[test]
fn numeric_expression_number_proof_does_not_escape_to_following_reads() {
    let before = compare(
        CompareOp::Lt,
        Expr::LocalGet(2),
        get(Expr::LocalGet(1), "value"),
    );
    let after = compare(CompareOp::Lt, Expr::LocalGet(2), Expr::LocalGet(1));
    let output = ir(vec![Stmt::Expr(before), Stmt::Expr(after)], true);
    let body = function(&output);
    let merge = body.find("rexpr.merge.").unwrap();
    assert!(
        body[merge..].contains("@js_rel_lt("),
        "unproven locals retain generic comparison after F/G"
    );
}

#[test]
fn numeric_expression_special_numbers_remain_ordered_native_comparisons() {
    for n in [f64::NAN, -0.0, f64::INFINITY, f64::NEG_INFINITY] {
        let output = ir(
            vec![Stmt::Expr(compare(
                CompareOp::Le,
                get(Expr::LocalGet(1), "value"),
                Expr::Number(n),
            ))],
            true,
        );
        assert!(function(&output).contains("fcmp ole double"));
    }
    let bad = f64::from_bits(0xfffd_0000_0000_0001);
    let output = ir(
        vec![Stmt::Expr(compare(
            CompareOp::Lt,
            get(Expr::LocalGet(1), "value"),
            Expr::Number(bad),
        ))],
        true,
    );
    assert!(!function(&output).contains("rexpr.fast"));
}
