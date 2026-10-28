use super::*;

#[test]
fn assignment_keys_evaluate_to_the_assigned_value() {
    let env = empty_env();
    let inner = Expr::IndexSet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::String("a".into())),
        value: Box::new(Expr::Integer(0)),
    };
    assert_eq!(infer_expr_type(&inner, &env), Type::Number);
    let outer = Expr::IndexSet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(inner),
        value: Box::new(Expr::String("a".into())),
    };
    assert_eq!(infer_expr_type(&outer, &env), Type::String);
}

#[test]
fn enum_image_computed_reads_use_the_existing_member_shape() {
    let env = empty_env();
    let image = Expr::EnumImage {
        class_name: "E".into(),
        stores: vec![
            ("a".into(), EnumValue::Number(0.0)),
            ("0".into(), EnumValue::String("a".into())),
        ],
        strict: true,
    };
    let named = Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(image.clone()),
        index: Box::new(Expr::String("a".into())),
    };
    assert_eq!(infer_expr_type(&named, &env), Type::Number);
    let indexed = Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(image),
        index: Box::new(Expr::LocalGet(0)),
    };
    let Type::Union(members) = infer_expr_type(&indexed, &env) else {
        panic!("enum members were lost")
    };
    assert_eq!(members.len(), 2);
    assert!(members.contains(&Type::Number) && members.contains(&Type::String));
}

#[test]
fn rest_array_fact_survives_only_without_reassignment() {
    for (source, expected) in [
        (
            "function f(...values) { return values; }",
            Type::Array(Box::new(Type::Any)),
        ),
        (
            "function f(...values) { values = new Date(); return values; }",
            Type::Any,
        ),
        (
            "function f(...values) { const set = () => { values = new Date(); }; return values; }",
            Type::Any,
        ),
    ] {
        let ast = perry_parser::parse_typescript(source, "rest.js").unwrap();
        let module = crate::lower_module(&ast, "rest.js", "rest.js").unwrap();
        let function = module.functions.iter().find(|f| f.name == "f").unwrap();
        let env = HirTypeEnv::from_module(&module);
        assert_eq!(
            env.local_type(function.params[0].id),
            Some(&expected),
            "{source}"
        );
    }
}

#[test]
fn private_array_length_keeps_the_numeric_generator_cursor() {
    for declaration in ["#items: unknown[] = [];", "#items = [];"] {
        let source = format!("class C {{ {declaration} *[Symbol.iterator]() {{ for (let cursor = this.#items.length - 1; cursor >= 0; cursor--) yield this.#items[cursor]; }} }}");
        let ast = perry_parser::parse_typescript(&source, "private.ts").unwrap();
        let module = crate::lower_module(&ast, "private.ts", "private.ts").unwrap();
        let env = HirTypeEnv::from_module(&module);
        let method = &module
            .classes
            .iter()
            .find(|c| c.name == "C")
            .unwrap()
            .computed_members[0]
            .function;
        fn visit(expr: &Expr, env: &HirTypeEnv, checked: &mut bool) {
            if let Expr::IndexGet { index, .. } = expr {
                assert_eq!(infer_expr_type(index, env), Type::Number);
                *checked = true;
            }
            crate::walker::walk_expr_children(expr, &mut |child| visit(child, env, checked));
        }
        let mut checked = false;
        for stmt in &method.body {
            crate::walker::stmt_any_expr(stmt, &mut |expr| {
                visit(expr, &env, &mut checked);
                false
            });
        }
        assert!(checked, "generator element read must be visited");
    }
}
