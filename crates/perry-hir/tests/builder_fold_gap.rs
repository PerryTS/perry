//! #10353: a builder whose assignments do not IMMEDIATELY follow the `{}`
//! binding must still fold into the literal — `const o = {}; const X = 1;
//! o.a = X;` is the ordinary shape of initialisation code, and leaving it
//! unfolded costs 75× (a 0-field anon shape that every store then transitions
//! dynamically through `js_put_value_set`).
//!
//! The gap is only skippable while moving the ALLOCATION below it stays
//! unobservable, so these tests pin both directions: the fold happens for
//! inert statements, and it does NOT happen for a statement that names the
//! binding, can run user code, or destructures.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "builder_fold_gap.ts", &mut cache)
        .expect("parse should succeed");
    lower_module(&parsed.module, "test", "builder_fold_gap.ts").expect("lower should succeed")
}

/// Number of constructor args on the `__AnonShape_…` allocation bound to
/// `name`, i.e. how many properties the literal carries after folding.
fn anon_shape_arity(stmts: &[Stmt], name: &str) -> Option<usize> {
    stmts.iter().find_map(|stmt| match stmt {
        Stmt::Let {
            name: binding,
            init: Some(Expr::New {
                class_name, args, ..
            }),
            ..
        } if binding == name && class_name.starts_with("__AnonShape_") => Some(args.len()),
        _ => None,
    })
}

fn runtime_set_keys(stmts: &[Stmt]) -> Vec<String> {
    stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Expr(Expr::PutValueSet { key, .. }) => match key.as_ref() {
                Expr::String(k) => Some(k.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn fn_body<'a>(module: &'a perry_hir::Module, name: &str) -> &'a [Stmt] {
    module
        .functions
        .iter()
        .find(|f| f.name == name)
        .map(|f| f.body.as_slice())
        .unwrap_or_else(|| panic!("function `{name}` not found"))
}

/// Index of the `Let` binding `name` within a statement list.
fn let_position(stmts: &[Stmt], name: &str) -> Option<usize> {
    stmts
        .iter()
        .position(|stmt| matches!(stmt, Stmt::Let { name: binding, .. } if binding == name))
}

#[test]
fn a_constant_binding_between_the_literal_and_its_stores_still_folds() {
    // The issue's `vA.ts`, at module scope.
    let module = lower_src(
        r#"
        const o: any = {};
        const x = 1;
        o.p1 = x; o.p2 = x; o.p3 = x; o.p4 = x; o.p5 = x; o.p6 = x;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(6),
        "all six stores should have folded into the allocation: {:?}",
        module.init
    );
    assert!(
        runtime_set_keys(&module.init).is_empty(),
        "no store should survive as a dynamic [[Set]]: {:?}",
        runtime_set_keys(&module.init)
    );
}

#[test]
fn the_skipped_binding_is_still_initialized_before_the_literal() {
    // Sinking the allocation below `const x` is the whole point: the folded
    // values read `x`, so `x` must still be initialized first or the literal
    // would hit `x`'s TDZ.
    let module = lower_src(
        r#"
        const o: any = {};
        const x = 1;
        o.p1 = x;
        "#,
    );

    let x = let_position(&module.init, "x").expect("`x` binding not found");
    let o = let_position(&module.init, "o").expect("`o` binding not found");
    assert!(
        x < o,
        "`x` must be initialized before the folded literal: {:?}",
        module.init
    );
}

#[test]
fn a_gap_inside_a_function_body_folds_too() {
    let module = lower_src(
        r#"
        export function build(): any {
            const o: any = {};
            const x = 1;
            o.p1 = x; o.p2 = x;
            return o;
        }
        "#,
    );

    let body = fn_body(&module, "build");
    assert_eq!(
        anon_shape_arity(body, "o"),
        Some(2),
        "both stores should have folded inside the function body: {body:?}"
    );
    assert!(
        runtime_set_keys(body).is_empty(),
        "no store should survive as a dynamic [[Set]]: {:?}",
        runtime_set_keys(body)
    );
}

#[test]
fn several_skipped_bindings_are_all_kept_in_order() {
    let module = lower_src(
        r#"
        const o: any = {};
        const a = 1;
        const b = a + 1;
        o.p1 = a;
        o.p2 = b;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(2),
        "both stores should have folded: {:?}",
        module.init
    );
    let a = let_position(&module.init, "a").expect("`a` binding not found");
    let b = let_position(&module.init, "b").expect("`b` binding not found");
    let o = let_position(&module.init, "o").expect("`o` binding not found");
    assert!(
        a < b && b < o,
        "the skipped bindings must keep their order and precede the literal: {:?}",
        module.init
    );
}

#[test]
fn a_second_builder_in_the_gap_folds_as_well() {
    let module = lower_src(
        r#"
        const a: any = {};
        const b: any = {};
        a.x = 1;
        b.y = 2;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "a"),
        Some(1),
        "the outer builder should fold: {:?}",
        module.init
    );
    assert_eq!(
        anon_shape_arity(&module.init, "b"),
        Some(1),
        "the builder found in the gap should fold too: {:?}",
        module.init
    );
}

#[test]
fn a_gap_statement_that_names_the_binding_blocks_the_fold() {
    // `alias` reads `o` before the allocation would happen — sinking the
    // declaration past it would be a TDZ ReferenceError.
    let module = lower_src(
        r#"
        const o: any = {};
        const alias = o;
        o.p1 = 1;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(0),
        "the allocation must stay empty: {:?}",
        module.init
    );
    assert_eq!(
        runtime_set_keys(&module.init),
        vec!["p1".to_string()],
        "the store must survive as a dynamic [[Set]]: {:?}",
        module.init
    );
}

#[test]
fn a_gap_statement_that_can_run_user_code_blocks_the_fold() {
    // The gap statement does not name `o`, but `peek()` reads it — sinking
    // the allocation below the call would turn that read into a TDZ
    // ReferenceError.
    let module = lower_src(
        r#"
        function peek(): any { return o; }
        const o: any = {};
        const seen = peek();
        o.p1 = 1;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(0),
        "a call in the gap must block the fold: {:?}",
        module.init
    );
    assert_eq!(
        runtime_set_keys(&module.init),
        vec!["p1".to_string()],
        "the store must survive as a dynamic [[Set]]: {:?}",
        module.init
    );
}

#[test]
fn a_destructuring_gap_blocks_the_fold() {
    // The pattern itself performs property reads, which can run a getter.
    let module = lower_src(
        r#"
        const src: any = { a: 1 };
        const o: any = {};
        const { a } = src;
        o.p1 = a;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(0),
        "a destructuring gap must block the fold: {:?}",
        module.init
    );
    assert_eq!(
        runtime_set_keys(&module.init),
        vec!["p1".to_string()],
        "the store must survive as a dynamic [[Set]]: {:?}",
        module.init
    );
}

#[test]
fn a_member_read_in_the_gap_blocks_the_fold() {
    // A getter on `src` could reach the binding.
    let module = lower_src(
        r#"
        const src: any = { a: 1 };
        const o: any = {};
        const a = src.a;
        o.p1 = a;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(0),
        "a member read in the gap must block the fold: {:?}",
        module.init
    );
    assert_eq!(
        runtime_set_keys(&module.init),
        vec!["p1".to_string()],
        "the store must survive as a dynamic [[Set]]: {:?}",
        module.init
    );
}

#[test]
fn a_populated_literal_does_not_skip_a_gap() {
    // Sinking `{ a: y }` below `const y` would turn a TDZ ReferenceError into
    // a successful build, so only empty literals may skip statements.
    let module = lower_src(
        r#"
        const o: any = { a: 1 };
        const y = 2;
        o.b = y;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(1),
        "the populated literal must keep its single property: {:?}",
        module.init
    );
    assert_eq!(
        runtime_set_keys(&module.init),
        vec!["b".to_string()],
        "the store must survive as a dynamic [[Set]]: {:?}",
        module.init
    );
}

#[test]
fn an_adjacent_builder_still_folds_into_a_populated_literal() {
    // The pre-#10353 behaviour is unchanged when there is no gap.
    let module = lower_src(
        r#"
        const o: any = { a: 1 };
        o.b = 2;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(2),
        "an adjacent store should still fold: {:?}",
        module.init
    );
    assert!(
        runtime_set_keys(&module.init).is_empty(),
        "no store should survive as a dynamic [[Set]]: {:?}",
        runtime_set_keys(&module.init)
    );
}

#[test]
fn a_type_only_declaration_in_the_gap_is_skipped() {
    let module = lower_src(
        r#"
        const o: any = {};
        type Width = number;
        interface Shape { w: Width }
        const x: Width = 1;
        o.p1 = x;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(1),
        "erased declarations should not block the fold: {:?}",
        module.init
    );
}

#[test]
fn an_enum_in_the_gap_blocks_the_fold() {
    // Unlike `type`/`interface`, an enum emits an initializer at run time.
    let module = lower_src(
        r#"
        const o: any = {};
        enum Color { Red }
        o.p1 = 1;
        "#,
    );

    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(0),
        "an enum in the gap must block the fold: {:?}",
        module.init
    );
}
