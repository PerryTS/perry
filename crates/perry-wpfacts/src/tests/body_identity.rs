#[test]
fn a_copied_class_parent_temp_does_not_rename_its_callers_binding() {
    let ast = perry_parser::parse_typescript(
        "class Base {} const Current = Base; function mixin(Parent: any) { return class extends Parent {}; } const Derived = mixin(Current); console.log(new Derived() instanceof Base);",
        "parent.ts",
    ).unwrap();
    let mut module = perry_hir::lower_module(&ast, "parent", "parent.ts").unwrap();
    let caller = module
        .init
        .iter()
        .find_map(|s| match s {
            perry_hir::Stmt::Let { id, name, .. } if name == "Current" => Some(*id),
            _ => None,
        })
        .expect("caller binding");
    assert_eq!(caller, 0, "exercise the old zero-based scratch allocator");
    let temp = perry_hir::lexical_frames::max_ids(&module).0 + 1;
    let class = module
        .classes
        .iter_mut()
        .find(|c| c.extends_expr.is_some())
        .unwrap();
    let value = class.extends_expr.take().unwrap();
    class.extends_expr = Some(Box::new(perry_hir::Expr::ScopedTemp {
        id: temp,
        value,
        body: Box::new(perry_hir::Expr::LocalGet(temp)),
    }));
    let template = class.name.clone();
    let factory = module
        .functions
        .iter_mut()
        .find(|f| f.name == "mixin")
        .unwrap();
    factory.body = vec![perry_hir::Stmt::Return(Some(perry_hir::Expr::ClassRef(
        template,
    )), 0)];
    perry_transform::inline_functions(
        &mut module,
        &Default::default(),
        &Default::default(),
        &Default::default(),
        &Default::default(),
    );
    let copy = module
        .classes
        .iter()
        .find(|c| c.name.contains("__inline_"))
        .expect("class copy must be live");
    let perry_hir::Expr::ScopedTemp { id, value, .. } = copy.extends_expr.as_deref().unwrap()
    else {
        panic!("parent evaluation must remain single");
    };
    assert_ne!(*id, caller);
    assert!(
        matches!(value.as_ref(), perry_hir::Expr::LocalGet(id) if *id == caller),
        "a private temp must not capture/rename its caller's free binding: {value:?}"
    );
}
