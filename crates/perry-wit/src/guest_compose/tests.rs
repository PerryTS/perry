use super::*;
use wac_graph::types::{Type, Types, ValueType};

#[test]
fn concrete_resource_future_payload_is_exported_with_its_enclosing_interface() {
    let api = r#"(type $api (instance
        (export "item" (type $item (sub resource)))
        (type $own (own $item))
        (type $record (record (field "owner" $own) (field "text" string)))
        (export "payload" (type $payload (eq $record)))
        (type $future (future $payload))
        (export "pending" (type $pending (eq $future)))
        (export "[constructor]item" (func (result $own)))
        (export "make" (func (result $pending)))))"#;
    let producer = wat::parse_str(
        r#"(component
        (import "kick" (func $kick async))
        (component $implementation
            (type $rep (resource (rep i32)))
            (export $item "item" (type $rep))
            (type $own (own $item))
            (type $record (record (field "owner" $own) (field "text" string)))
            (export $payload "payload" (type $record))
            (type $future (future $payload))
            (export $pending "pending" (type $future))
            (core module $m
                (memory (export "memory") 1)
                (func (export "make") (result i32) i32.const 0)
                (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 0))
            (core instance $m (instantiate $m))
            (alias core export $m "memory" (core memory $memory))
            (alias core export $m "make" (core func $make))
            (alias core export $m "realloc" (core func $realloc))
            (func (export "make") (result $pending)
                (canon lift (core func $make) (memory $memory) (realloc $realloc)))
            (func (export "[constructor]item") (result $own) (canon lift (core func $make))))
        (instance $api (instantiate $implementation))
        (export "api" (instance $api))
        (export "actor" (func $kick))
        (export "signal" (func $kick)))"#,
    )
    .unwrap();
    Validator::new_with_features(WasmFeatures::all())
        .validate_all(&producer)
        .expect("valid concrete-resource fixture");
    let facade = wat::parse_str(format!(
        r#"(component {api}
        (import "source" (instance $source (type $api)))
        (import "kick" (func async))
        (alias export $source "make" (func $make))
        (alias export $source "[constructor]item" (func $ctor))
        (instance $forward (export "forward" (func $make)) (export "new" (func $ctor)))
        (export "forwarder" (instance $forward)))"#
    ))
    .unwrap();
    let plan = serde_json::to_vec(&serde_json::json!({
        "schema_version":1, "actor":"actor", "signal":"signal", "forward_interface":"forwarder",
        "imports":[{"local":"source","original":"api"},{"local":"kick","original":"actor"}],
        "forwards":[{"interface":"api","function":"make","export":"forward"},
                    {"interface":"api","function":"[constructor]item","export":"new"}]
    }))
    .unwrap();
    let bytes = compose_experimental_guest(&producer, &facade, &plan).unwrap();
    let mut types = Types::default();
    let package = Package::from_bytes("test:concrete", None, bytes, &mut types).unwrap();
    let exports = &types[package.ty()].exports;
    assert_eq!(exports.len(), 1);
    let ItemKind::Instance(api) = exports["api"] else {
        panic!("public interface")
    };
    assert_eq!(types[api].exports.len(), 5);
    let ItemKind::Func(make) = types[api].exports["make"] else {
        panic!("sync factory")
    };
    assert!(!types[make].is_async);
}

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
        (instance $forward (export "forward" (func $make)))
        (export "forwarder" (instance $forward)))"#
    ))
    .unwrap();
    let plan = serde_json::to_vec(&serde_json::json!({
        "schema_version":1, "actor":"actor", "signal":"signal", "forward_interface":"forwarder",
        "imports":[{"local":"source","original":"api"},{"local":"kick","original":"actor"}],
        "forwards":[{"interface":"api","function":"make","export":"forward"}]
    }))
    .unwrap();
    let bytes = compose_experimental_guest(&producer, &facade, &plan).unwrap();
    let mut missing: serde_json::Value = serde_json::from_slice(&plan).unwrap();
    missing["forwards"] = serde_json::json!([]);
    let error =
        compose_experimental_guest(&producer, &facade, &serde_json::to_vec(&missing).unwrap())
            .unwrap_err();
    assert!(error.to_string().contains("synchronous facade function"));
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
