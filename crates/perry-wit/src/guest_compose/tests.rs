use super::*;
use wac_graph::types::{Type, Types, ValueType};

#[test]
fn sibling_facade_rebuilds_public_interfaces_without_replacing_their_resource_identity() {
    let interface = r#"(type $api (instance
        (export "item" (type $item (sub resource)))
        (type $owned (own $item))
        (export "make" (func (result $owned)))
        (export "later" (func async (result u32)))))"#;
    let producer = wat::parse_str(format!(
        r#"(component {interface}
        (import "api" (instance $api (type $api)))
        (import "start" (func $kick async))
        (export "api" (instance $api))
        (export "actor" (func $kick))
        (export "signal" (func $kick)))"#
    ))
    .unwrap();
    let facade = wat::parse_str(format!(
        r#"(component {interface}
        (import "source" (instance $api (type $api)))
        (import "kick" (func async))
        (alias export $api "make" (func $make))
        (export "forward" (func $make)))"#
    ))
    .unwrap();
    let plan = serde_json::to_vec(&serde_json::json!({
        "schema_version":1, "actor":"actor", "signal":"signal",
        "imports":[{"local":"source","original":"api"},{"local":"kick","original":"actor"}],
        "forwards":[{"interface":"api","function":"make","export":"forward"}]
    }))
    .unwrap();
    let bytes = compose_experimental_guest(&producer, &facade, &plan).unwrap();
    let mut types = Types::default();
    let package = Package::from_bytes("test:composed", None, bytes, &mut types).unwrap();
    let exports = &types[package.ty()].exports;
    assert_eq!(
        exports.len(),
        1,
        "private actor and temporary exports must stay hidden"
    );
    let ItemKind::Instance(api) = exports["api"] else {
        panic!("public API instance")
    };
    let exports = &types[api].exports;
    let ItemKind::Type(Type::Resource(resource)) = exports["item"] else {
        panic!("resource alias")
    };
    let ItemKind::Func(make) = exports["make"] else {
        panic!("sync factory")
    };
    assert!(
        !types[make].is_async,
        "the facade must preserve the synchronous factory"
    );
    assert_eq!(types[make].result, Some(ValueType::Own(resource)));
    let ItemKind::Func(later) = exports["later"] else {
        panic!("async function")
    };
    assert!(types[later].is_async);
}
