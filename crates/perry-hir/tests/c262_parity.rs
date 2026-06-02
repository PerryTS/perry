use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, ArrayElement, BinaryOp, Expr, Function, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> perry_hir::Module {
    lower_src_with_filename(src, "c262_parity.ts")
}

fn lower_js_src(src: &str) -> perry_hir::Module {
    lower_src_with_filename(src, "c262_parity.js")
}

fn lower_src_with_filename(src: &str, filename: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed =
        parse_typescript_with_cache(src, filename, &mut cache).expect("parse should succeed");
    lower_module(&parsed.module, "test", filename).expect("lower should succeed")
}

fn top_level_init<'a>(module: &'a perry_hir::Module, name: &str) -> &'a Expr {
    module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name: binding,
                init: Some(init),
                ..
            } if binding == name => Some(init),
            _ => None,
        })
        .unwrap_or_else(|| panic!("top-level binding `{name}` not found"))
}

fn top_level_local_id(module: &perry_hir::Module, name: &str) -> perry_types::LocalId {
    module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                id, name: binding, ..
            } if binding == name => Some(*id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("top-level binding `{name}` not found"))
}

fn function<'a>(module: &'a perry_hir::Module, name: &str) -> &'a Function {
    module
        .functions
        .iter()
        .find(|func| func.name == name)
        .unwrap_or_else(|| panic!("function `{name}` not found"))
}

fn is_number_literal(expr: &Expr, expected: f64) -> bool {
    match expr {
        Expr::Number(actual) => *actual == expected,
        Expr::Integer(actual) => (*actual as f64) == expected,
        _ => false,
    }
}

#[test]
fn direct_eval_constant_addition_with_test262_whitespace_folds() {
    let module = lower_src("const folded = eval(\"1\\u0009+\\u00091\");");

    assert!(matches!(
        top_level_init(&module, "folded"),
        Expr::Number(n) if *n == 2.0
    ));
}

#[test]
fn direct_eval_simple_assignment_updates_captured_var_binding() {
    let module = lower_src(
        r#"
        var a;
        function foo() {
          eval("a = 10");
          return () => a;
        }
        "#,
    );
    let a_id = top_level_local_id(&module, "a");
    let foo = function(&module, "foo");

    assert!(
        matches!(
            foo.body.first(),
            Some(Stmt::Expr(Expr::LocalSet(id, value)))
                if *id == a_id && is_number_literal(value, 10.0)
        ),
        "{:?}",
        foo.body
    );
}

#[test]
fn arrow_default_parameter_self_reference_throws_reference_error() {
    let module = lower_src("var f = (x = x) => { return 1; };");
    let Expr::Closure { body, .. } = top_level_init(&module, "f") else {
        panic!("expected arrow closure");
    };
    let Some(Stmt::If { then_branch, .. }) = body.first() else {
        panic!("expected default-parameter guard, got {body:?}");
    };

    let throws_reference_error = match then_branch.as_slice() {
        [Stmt::Throw(Expr::ReferenceErrorNew(_))] => true,
        [Stmt::Throw(Expr::Call { callee, .. })] => matches!(
            callee.as_ref(),
            Expr::ExternFuncRef { name, .. } if name.starts_with("js_throw_")
        ),
        _ => false,
    };
    assert!(throws_reference_error, "{then_branch:?}");
}

#[test]
fn arrow_default_parameter_eval_var_conflict_throws_syntax_error() {
    let module = lower_src(r#"var f = (a = eval("var a = 42")) => { return 1; };"#);
    let Expr::Closure { body, .. } = top_level_init(&module, "f") else {
        panic!("expected arrow closure");
    };
    let Some(Stmt::If { then_branch, .. }) = body.first() else {
        panic!("expected default-parameter guard, got {body:?}");
    };

    assert!(
        matches!(
            then_branch.as_slice(),
            [Stmt::Throw(Expr::SyntaxErrorNew(_))]
        ),
        "{then_branch:?}"
    );
}

#[test]
fn array_elisions_lower_as_holes_not_undefined_values() {
    let module = lower_src("const arr = [1, , 2];");

    let Expr::ArraySpread(elements) = top_level_init(&module, "arr") else {
        panic!("array with elision should use spread-aware element representation");
    };
    assert_eq!(elements.len(), 3, "{elements:?}");
    assert!(
        matches!(&elements[0], ArrayElement::Expr(expr) if is_number_literal(expr, 1.0)),
        "{elements:?}"
    );
    assert!(matches!(elements[1], ArrayElement::Hole), "{elements:?}");
    assert!(
        matches!(&elements[2], ArrayElement::Expr(expr) if is_number_literal(expr, 2.0)),
        "{elements:?}"
    );
}

#[test]
fn sloppy_assignment_expression_creates_storage_before_following_getvalue() {
    let module = lower_src("const result = (y = 1) + y;");
    let y_id = module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                id,
                name,
                init: Some(Expr::Undefined),
                ..
            } if name == "y" => Some(*id),
            _ => None,
        })
        .expect("sloppy assignment target should be predeclared");

    let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = top_level_init(&module, "result")
    else {
        panic!("result should lower as addition");
    };

    assert!(
        matches!(left.as_ref(), Expr::LocalSet(id, value) if *id == y_id && is_number_literal(value, 1.0)),
        "{left:?}"
    );
    assert!(matches!(right.as_ref(), Expr::LocalGet(id) if *id == y_id));
}

#[test]
fn sloppy_assignment_in_if_test_creates_storage_before_following_getvalue() {
    let module = lower_src("if ((y = 1) + y !== 2) { throw new Error('bad'); }");
    let y_id = module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                id,
                name,
                init: Some(Expr::Undefined),
                ..
            } if name == "y" => Some(*id),
            _ => None,
        })
        .expect("sloppy assignment target in if test should be predeclared");

    let Some(Stmt::If { condition, .. }) = module
        .init
        .iter()
        .find(|stmt| matches!(stmt, Stmt::If { .. }))
    else {
        panic!("expected lowered if statement, got {:?}", module.init);
    };

    let Expr::Compare { left, .. } = condition else {
        panic!("if test should lower as comparison, got {condition:?}");
    };
    let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = left.as_ref()
    else {
        panic!("comparison lhs should lower as addition, got {left:?}");
    };

    assert!(
        matches!(left.as_ref(), Expr::LocalSet(id, value) if *id == y_id && is_number_literal(value, 1.0)),
        "{left:?}"
    );
    assert!(matches!(right.as_ref(), Expr::LocalGet(id) if *id == y_id));
}

#[test]
fn sloppy_js_yield_identifier_arrow_parameters_lower() {
    let module = lower_js_src(
        r#"
        var yield = 23;
        var f = (x = yield) => x;
        var g = yield => yield;
        var h = (yield) => yield;
        "#,
    );

    assert!(matches!(top_level_init(&module, "f"), Expr::Closure { .. }));
    assert!(matches!(top_level_init(&module, "g"), Expr::Closure { .. }));
    assert!(matches!(top_level_init(&module, "h"), Expr::Closure { .. }));
}
