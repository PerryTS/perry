use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "class_self_new_shadowing.ts", &mut cache)
        .expect("parse should succeed");
    lower_module(&parsed.module, "test", "class_self_new_shadowing.ts")
        .expect("lowering should succeed")
}

#[test]
fn class_self_new_wins_over_same_named_outer_local() {
    let module = lower_src(
        r#"
        var h;
        const factory = () => {
            const captured = "factory";
            class h {
                constructor() { this.value = captured; }
                static instance() { return new h(); }
            }
            return h;
        };
        "#,
    );

    let class = module
        .classes
        .iter()
        .find(|class| class.name == "h")
        .expect("factory-local class h should be lowered");
    let instance = class
        .static_methods
        .iter()
        .find(|method| method.name == "instance")
        .expect("static instance method should be lowered");

    assert!(
        instance.body.iter().any(|stmt| matches!(
            stmt,
            Stmt::Return(Some(Expr::New { class_name, .. })) if class_name == "h"
        )),
        "class self-construction must bind to the class, not the outer local: {:#?}",
        instance.body
    );
}

#[test]
fn collision_renamed_class_self_new_uses_unique_class_name() {
    let module = lower_src(
        r#"
        var h;
        const first = () => {
            class h { static instance() { return new h(); } }
            return h;
        };
        const second = () => {
            class h { static instance() { return new h(); } }
            return h;
        };
        "#,
    );

    let class = module
        .classes
        .iter()
        .find(|class| class.name.starts_with("h$"))
        .expect("second class h should receive a unique registration name");
    let instance = class
        .static_methods
        .iter()
        .find(|method| method.name == "instance")
        .expect("static instance method should be lowered");

    assert!(
        instance.body.iter().any(|stmt| matches!(
            stmt,
            Stmt::Return(Some(Expr::New { class_name, .. })) if class_name == &class.name
        )),
        "renamed class self-construction must use its unique name: {:#?}",
        instance.body
    );
}

#[test]
fn method_parameter_shadows_class_self_name() {
    let module = lower_src(
        r#"
        class C {
            static make(C) { return new C(); }
        }
        "#,
    );

    let class = module
        .classes
        .iter()
        .find(|class| class.name == "C")
        .expect("class C should be lowered");
    let make = class
        .static_methods
        .iter()
        .find(|method| method.name == "make")
        .expect("static make method should be lowered");
    let parameter_id = make.params.first().expect("C parameter should exist").id;

    assert!(
        make.body.iter().any(|stmt| matches!(
            stmt,
            Stmt::Return(Some(Expr::NewDynamic { callee, .. }))
                if matches!(callee.as_ref(), Expr::LocalGet(id) if *id == parameter_id)
        )),
        "method parameter C must shadow the class's inner name: {:#?}",
        make.body
    );
}

/// A method-LOCAL binding is a different lowering path from a parameter: the
/// local is introduced by a `Stmt::Let` inside the body rather than by the
/// method signature, so a fix that only consults the parameter list would
/// pass the test above and still resolve `new C()` to the lexical class.
#[test]
fn method_local_declaration_shadows_class_self_name() {
    let module = lower_src(
        r#"
        function factory() { return { made: true }; }
        class C {
            static make() {
                const C = factory;
                return new C();
            }
        }
        "#,
    );

    let class = module
        .classes
        .iter()
        .find(|class| class.name == "C")
        .expect("class C should be lowered");
    let make = class
        .static_methods
        .iter()
        .find(|method| method.name == "make")
        .expect("static make method should be lowered");
    let local_id = make
        .body
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let { id, name, .. } if name == "C" => Some(*id),
            _ => None,
        })
        .expect("the `const C` local should be lowered");

    assert!(
        make.body.iter().any(|stmt| matches!(
            stmt,
            Stmt::Return(Some(Expr::NewDynamic { callee, .. }))
                if matches!(callee.as_ref(), Expr::LocalGet(id) if *id == local_id)
        )),
        "method-local C must shadow the class's inner name: {:#?}",
        make.body
    );
}

#[test]
fn named_class_expression_self_new_uses_unique_class_name() {
    let module = lower_src(
        r#"
        class h {}
        const value = class h { static instance() { return new h(); } };
        "#,
    );

    let class = module
        .classes
        .iter()
        .find(|class| class.name == "value")
        .expect("named class expression should use its unique binding registration name");
    let instance = class
        .static_methods
        .iter()
        .find(|method| method.name == "instance")
        .expect("static instance method should be lowered");

    assert!(
        instance.body.iter().any(|stmt| matches!(
            stmt,
            Stmt::Return(Some(Expr::New { class_name, .. })) if class_name == &class.name
        )),
        "named class-expression self-construction must use its unique name: {:#?}",
        instance.body
    );
}
