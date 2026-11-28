//! A class's own inner-name binding is in scope for everything lexically
//! inside the class body, nested classes included (ClassDefinitionEvaluation
//! creates classScope before ClassHeritage and the element functions close
//! over it). esbuild emits `var E = class _E { … }`; a nested
//! `class X extends _E {}` inside a method must resolve `_E` to the outer
//! class, not fail as an unbound global (VS Code
//! `EditorCommand.bindToContribution`).

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Class, Expr, Module};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "nested_heritage.js", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "nested_heritage.js")
                .expect("lowering should succeed")
        })
        .unwrap()
        .join()
        .unwrap()
}

fn class<'a>(module: &'a Module, source_name: &str) -> &'a Class {
    module
        .classes
        .iter()
        .find(|c| c.name == source_name || c.name.starts_with(&format!("{source_name}__")))
        .unwrap_or_else(|| {
            panic!(
                "no class {source_name}: {:?}",
                module.classes.iter().map(|c| &c.name).collect::<Vec<_>>()
            )
        })
}

/// The heritage of `child` is the class `parent`: statically or as the
/// evaluated class reference.
fn extends_class(child: &Class, parent: &Class) -> bool {
    child.extends == Some(parent.id)
        || matches!(child.extends_expr.as_deref(), Some(Expr::ClassRef(n)) if *n == parent.name)
}

fn outer_of(module: &Module, nested: &[&str]) -> Class {
    let others: Vec<_> = module
        .classes
        .iter()
        .filter(|c| !nested.iter().any(|n| c.name == *n || c.name.starts_with(&format!("{n}__"))))
        .collect();
    assert_eq!(others.len(), 1, "one outer class expected");
    others[0].clone()
}

#[test]
fn nested_class_extends_outer_inner_name_in_static_method() {
    let m = lower("var E = class _E { static make() { return class X extends _E {}; } };");
    let outer = outer_of(&m, &["X"]);
    assert!(extends_class(class(&m, "X"), &outer));
}

#[test]
fn nested_class_extends_outer_inner_name_in_instance_method() {
    let m = lower("var E = class _E { make() { return class X extends _E {}; } };");
    let outer = outer_of(&m, &["X"]);
    assert!(extends_class(class(&m, "X"), &outer));
}

#[test]
fn nested_class_extends_outer_inner_name_in_static_field_arrow() {
    let m = lower("var E = class _E { static f = () => class Y extends _E {}; };");
    let outer = outer_of(&m, &["Y"]);
    assert!(extends_class(class(&m, "Y"), &outer));
}

#[test]
fn nested_class_body_reads_outer_inner_name() {
    let m = lower("var E = class _E { static s() { return class Z { m() { return _E; } }; } };");
    let outer = outer_of(&m, &["Z"]);
    let z = class(&m, "Z");
    let body = format!("{:?}", z.methods);
    assert!(body.contains(&format!("ClassRef({:?})", outer.name)), "{body}");
}

#[test]
fn local_inside_method_shadows_outer_inner_name_in_heritage() {
    let m = lower(
        "class Other {}\n\
         var E = class _E { static make() { const _E = Other; return class X extends _E {}; } };",
    );
    let x = class(&m, "X");
    let outer = m
        .classes
        .iter()
        .find(|c| c.name != "Other" && !std::ptr::eq(*c, x))
        .unwrap();
    assert!(!extends_class(x, outer));
    assert!(x.extends_expr.is_some(), "the local `_E` is the evaluated parent");
}
