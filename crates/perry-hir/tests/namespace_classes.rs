//! Namespace class declarations need the same evaluation steps as module
//! classes, and the namespace itself is a plain object they are published on.

use perry_diagnostics::SourceCache;
use perry_hir::types::LocalId;
use perry_hir::{lower_module, Expr, Module, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower(source: &str) -> Module {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(source, "test.ts", &mut cache).expect("parse");
    lower_module(&parsed.module, "test", "test.ts").expect("lower")
}

/// The local a module-level `Let` binds `name` to.
fn binding(module: &Module, name: &str) -> LocalId {
    module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let { id, name: n, .. } if n == name => Some(*id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no binding {name}"))
}

/// The object and property an init statement stores to: a declaration's
/// publication (`PropertySet`) or an exported variable's assignment
/// (`PutValueSet`).
fn store(stmt: &Stmt) -> Option<(&Expr, &str, &Expr)> {
    let set = match stmt {
        Stmt::Expr(set) => set,
        // A nested exported namespace: `Inner = Outer.Inner || (Outer.Inner = {})`.
        Stmt::Let {
            init: Some(Expr::Logical { right, .. }),
            ..
        } => right.as_ref(),
        _ => return None,
    };
    match set {
        Expr::PropertySet {
            object,
            property,
            value,
            ..
        } => Some((object, property, value)),
        Expr::PutValueSet {
            target, key, value, ..
        } => match key.as_ref() {
            Expr::String(key) => Some((target, key, value)),
            _ => None,
        },
        _ => None,
    }
}

fn stores_on(object: &Expr, id: LocalId) -> bool {
    matches!(object, Expr::LocalGet(o) if *o == id)
}

/// The init statement storing `property` on the object in `object`.
fn publication<'m>(
    module: &'m Module,
    object: LocalId,
    property: &str,
) -> Option<(usize, &'m Expr)> {
    module.init.iter().enumerate().find_map(|(i, stmt)| {
        let (o, p, value) = store(stmt)?;
        (stores_on(o, object) && p == property).then_some((i, value))
    })
}

/// The properties stored on the object in `object`, in init order.
fn published(module: &Module, object: LocalId) -> Vec<String> {
    module
        .init
        .iter()
        .filter_map(store)
        .filter(|(o, _, _)| stores_on(o, object))
        .map(|(_, p, _)| p.to_string())
        .collect()
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
    assert!(module.classes.iter().all(|c| c.name != "N"));
    let n = binding(&module, "N");
    assert!(matches!(
        module.init.iter().find_map(|s| match s {
            Stmt::Let { id, init, .. } if *id == n => Some(init),
            _ => None,
        }),
        Some(Some(Expr::Object(props))) if props.is_empty()
    ));
    assert_eq!(published(&module, n), ["before", "C", "after"]);
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
    let (publication, value) = publication(&module, n, "C").expect("namespace publication");
    assert!(matches!(value, Expr::ClassRef(c) if c == "N.C"));
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
    for name in ["N", "N.Inner", "Other"] {
        assert!(module.classes.iter().all(|c| c.name != name), "{name}");
    }
    assert_eq!(published(&module, binding(&module, "N")), ["C", "Inner"]);
    assert_eq!(published(&module, binding(&module, "Inner")), ["C"]);
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
    let n = binding(&module, "N");
    let body = match publication(&module, n, "inside") {
        Some((_, Expr::Closure { body, .. })) => Some(body),
        _ => None,
    }
    .expect("sibling arrow");
    assert!(body.iter().any(|stmt| matches!(stmt,
        Stmt::Return(Some(Expr::ClassRef(name))) if name == "N.C"
    )));
}

#[test]
fn exported_variables_are_properties_of_the_namespace_object() {
    let module = lower(
        r#"
        let x = "outer";
        namespace N {
            export let x = 1;
            export function read() { return x; }
            export function shadow(x: number) { return x; }
        }
        "#,
    );
    let n = binding(&module, "N");
    assert_eq!(published(&module, n), ["x", "read", "shadow"]);
    let read = module
        .functions
        .iter()
        .find(|f| f.name == "N.read")
        .expect("N.read");
    assert!(matches!(read.body.as_slice(),
        [Stmt::Return(Some(Expr::PropertyGet { object, property, .. }))]
        if property == "x" && matches!(object.as_ref(), Expr::LocalGet(id) if *id == n)));
    let shadow = module
        .functions
        .iter()
        .find(|f| f.name == "N.shadow")
        .expect("N.shadow");
    assert!(matches!(shadow.body.as_slice(),
        [Stmt::Return(Some(Expr::LocalGet(id)))] if *id == shadow.params[0].id));
}
