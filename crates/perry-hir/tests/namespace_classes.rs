//! Namespace class declarations need the same evaluation steps as module classes.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Module, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower(source: &str) -> Module {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(source, "test.ts", &mut cache).expect("parse");
    lower_module(&parsed.module, "test", "test.ts").expect("lower")
}

#[test]
fn namespace_class_registers_parent_before_initializers_and_publication() {
    let module = lower(
        r#"
        function base() { return class {} }
        export namespace N {
            export const before = 1;
            export class C extends base() { static value = 7 }
            export const after = 2;
        }
        "#,
    );
    let namespace = module.classes.iter().find(|c| c.name == "N").unwrap();
    assert_eq!(
        namespace
            .static_fields
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["before", "C", "after"]
    );
    let parent = module.init.iter().position(|s| matches!(s,
        Stmt::Expr(Expr::RegisterClassParentDynamic { class_name, .. }) if class_name == "N.C"
    )).expect("dynamic parent registration");
    let initializer = module
        .init
        .iter()
        .position(|s| {
            matches!(s,
                Stmt::Expr(Expr::StaticFieldSet { class_name, field_name, .. })
                if class_name == "N.C" && field_name == "value"
            )
        })
        .expect("static initializer");
    let publication = module.init.iter().position(|s| matches!(s,
        Stmt::Expr(Expr::StaticFieldSet { class_name, field_name, value })
        if class_name == "N" && field_name == "C" && matches!(value.as_ref(), Expr::ClassRef(n) if n == "N.C")
    )).expect("namespace publication");
    assert!(parent < initializer && initializer < publication);
}

#[test]
fn namespace_class_names_are_lexical_and_private_classes_are_not_published() {
    let module = lower(
        r#"
        class C {}
        namespace N {
            export class C {}
            class Hidden { static value = 3 }
            export namespace Inner { export class C {} }
        }
        namespace Other { export class C {} }
        "#,
    );
    for name in ["C", "N.C", "N.Hidden", "N.Inner.C", "Other.C"] {
        assert_eq!(
            module.classes.iter().filter(|c| c.name == name).count(),
            1,
            "{name}"
        );
    }
    let namespace = module.classes.iter().find(|c| c.name == "N").unwrap();
    assert_eq!(
        namespace
            .static_fields
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["C", "Inner"]
    );
    assert!(module.init.iter().any(|s| matches!(s,
        Stmt::Expr(Expr::StaticFieldSet { class_name, field_name, .. })
        if class_name == "N.Hidden" && field_name == "value"
    )));
}

#[test]
fn namespace_class_value_resolves_inside_a_sibling_arrow() {
    let module = lower(
        r#"
        export namespace N {
            export const inside = () => C;
            export class C {}
        }
        "#,
    );
    let body = module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name,
                init: Some(Expr::Closure { body, .. }),
                ..
            } if name == "inside" => Some(body),
            _ => None,
        })
        .expect("sibling arrow");
    assert!(body.iter().any(|stmt| matches!(stmt,
        Stmt::Return(Some(Expr::ClassRef(name))) if name == "N.C"
    )));
}
