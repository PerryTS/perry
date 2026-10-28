//! Exercise the pinned encoder fix without an embedding callback or a resource
//! disposer. Root function types must retain the exact imported resource alias.
use wac_graph::{types::Package, CompositionGraph, EncodeOptions};

fn compose(text: &str) {
    let mut graph = CompositionGraph::new();
    let package = Package::from_bytes(
        "test:resource-imports",
        None,
        wat::parse_str(text).unwrap(),
        graph.types_mut(),
    )
    .unwrap();
    let package = graph.register_package(package).unwrap();
    let original = graph.instantiate(package);
    graph.export(original, "original").unwrap();
    let encoded = graph
        .encode(EncodeOptions {
            validate: true,
            ..Default::default()
        })
        .unwrap();
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    let engine = wasmtime::Engine::new(&config).unwrap();
    wasmtime::component::Component::from_binary(&engine, &encoded).unwrap();
}

#[test]
fn composed_root_borrows_and_owners_preserve_same_named_interface_resource_identities() {
    compose(
        r#"(component
        (import "left" (instance $left (export "item" (type (sub resource)))))
        (import "right" (instance $right (export "item" (type (sub resource)))))
        (alias export $left "item" (type $left-item))
        (alias export $right "item" (type $right-item))
        (import "borrow-left" (func (param "value" (borrow $left-item))))
        (import "borrow-right" (func (param "value" (borrow $right-item))))
        (import "own-left" (func (param "value" (own $left-item))))
        (import "own-right" (func (param "value" (own $right-item))))
    )"#,
    );
}

#[test]
fn composed_root_borrows_and_owners_accept_direct_resource_imports() {
    compose(
        r#"(component
        (import "item" (type $item (sub resource)))
        (import "loan" (func (param "value" (borrow $item))))
        (import "consume" (func (param "value" (own $item))))
    )"#,
    );
}

#[test]
fn composed_nested_functions_alias_outer_resources_without_type_reexports() {
    compose(
        r#"(component
        (import "left" (instance $left (export "item" (type (sub resource)))))
        (import "right" (instance $right (export "item" (type (sub resource)))))
        (alias export $left "item" (type $left-item))
        (alias export $right "item" (type $right-item))
        (import "factory" (instance
            (export "make-left" (func (result (own $left-item))))
            (export "make-right" (func (result (own $right-item))))
            (export "inspect" (func (param "left" (borrow $left-item)) (param "right" (borrow $right-item))))))
    )"#,
    );
}

#[test]
fn composed_root_direct_resource_does_not_shadow_an_unrelated_interface_resource() {
    compose(
        r#"(component
        (import "item" (type $direct (sub resource)))
        (import "items" (instance $items (export "item" (type (sub resource)))))
        (alias export $items "item" (type $indirect))
        (import "loan" (func (param "direct" (borrow $direct)) (param "indirect" (borrow $indirect))))
        (import "consume" (func (param "direct" (own $direct)) (param "indirect" (own $indirect))))
    )"#,
    );
}
