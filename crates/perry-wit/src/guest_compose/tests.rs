use super::*;
use wac_graph::types::{Type, Types, ValueType};

#[test]
fn exported_resource_records_are_shared_through_the_published_interface() {
    let producer = wat::parse_str(
        r#"(component
        (import "start" (func $kick async))
        (component $implementation
            (type $rep (resource (rep i32)))
            (export $item "item" (type $rep))
            (type $own (own $item))
            (type $record (record (field "owner" $own) (field "text" string)))
            (export $payload "payload" (type $record))
            (core module $m
                (memory (export "memory") 1)
                (func (export "echo") (param i32 i32 i32) (result i32) i32.const 0)
                (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 0))
            (core instance $m (instantiate $m))
            (alias core export $m "memory" (core memory $memory))
            (alias core export $m "echo" (core func $echo))
            (alias core export $m "realloc" (core func $realloc))
            (func (export "echo") (param "value" $payload) (result $payload)
                (canon lift (core func $echo) (memory $memory) (realloc $realloc))))
        (instance $api (instantiate $implementation))
        (export $published "api" (instance $api))
        (alias export $published "payload" (type $payload))
        (alias export $published "echo" (func $echo))
        (instance $mirror (export "payload" (type $payload)) (export "pass" (func $echo)))
        (export "mirror" (instance $mirror))
        (export "actor" (func $kick))
        (export "signal" (func $kick)))"#,
    )
    .unwrap();
    let facade = wat::parse_str(
        r#"(component
        (type $api (instance
            (export "item" (type $item (sub resource)))
            (type $own (own $item))
            (type $record (record (field "owner" $own) (field "text" string)))
            (export "payload" (type $payload (eq $record)))
            (export "echo" (func (param "value" $payload) (result $payload)))))
        (import "source-api" (instance $api (type $api)))
        (alias export $api "payload" (type $payload))
        (type $mirror (instance
            (alias outer 1 $payload (type $outer))
            (export "payload" (type $payload (eq $outer)))
            (export "pass" (func (param "value" $payload) (result $payload)))))
        (import "source-mirror" (instance $mirror (type $mirror)))
        (import "kick" (func async))
        (alias export $api "echo" (func $echo))
        (alias export $mirror "pass" (func $pass))
        (instance $forward (export "echo" (func $echo)) (export "pass" (func $pass)))
        (export "forwarder" (instance $forward)))"#,
    )
    .unwrap();
    Validator::new_with_features(WasmFeatures::all())
        .validate_all(&producer)
        .unwrap();
    let plan = serde_json::to_vec(&serde_json::json!({
        "schema_version":1, "actor":"actor", "signal":"signal", "forward_interface":"forwarder",
        "imports":[{"local":"source-api","original":"api"},{"local":"source-mirror","original":"mirror"},{"local":"kick","original":"actor"}],
        "forwards":[{"interface":"api","function":"echo","export":"echo"},
                    {"interface":"mirror","function":"pass","export":"pass"}]
    })).unwrap();
    let bytes = compose_experimental_guest(&producer, &facade, &plan).unwrap();
    let mut types = Types::default();
    let package = Package::from_bytes("test:shared-output", None, bytes, &mut types).unwrap();
    let exports = &types[package.ty()].exports;
    assert_eq!(exports.len(), 2);
    let ItemKind::Instance(api) = exports["api"] else {
        panic!("api")
    };
    let ItemKind::Instance(mirror) = exports["mirror"] else {
        panic!("mirror")
    };
    let ItemKind::Type(Type::Value(first)) = types[api].exports["payload"] else {
        panic!("api payload")
    };
    let ItemKind::Type(Type::Value(second)) = types[mirror].exports["payload"] else {
        panic!("shared payload")
    };
    // The parser can allocate distinct record entries after export aliasing.
    // Their structure and the underlying nominal resource must stay equal.
    let owner = |ty| {
        let ValueType::Defined(id) = types.resolve_value_type(ty) else {
            panic!("record type")
        };
        let wac_graph::types::DefinedType::Record(record) = &types[id] else {
            panic!("record payload")
        };
        assert_eq!(record.fields.len(), 2);
        assert_eq!(
            types.resolve_value_type(record.fields["text"]),
            ValueType::Primitive(wac_graph::types::PrimitiveType::String)
        );
        let ValueType::Own(resource) = types.resolve_value_type(record.fields["owner"]) else {
            panic!("owned nominal resource")
        };
        types.resolve_resource(resource)
    };
    let ItemKind::Type(Type::Resource(resource)) = types[api].exports["item"] else {
        panic!("public resource")
    };
    assert_eq!(owner(first), types.resolve_resource(resource));
    assert_eq!(owner(second), types.resolve_resource(resource));
}

#[test]
fn imported_resource_records_and_future_types_keep_shared_interface_bindings() {
    let foreign = r#"(type $foreign (instance
        (export "item" (type $item (sub resource)))
        (type $own (own $item))
        (type $record (record (field "owner" $own) (field "label" string)))
        (export "payload" (type $payload (eq $record)))))"#;
    let api = r#"(type $api (instance
        (alias outer 1 $payload (type $outer))
        (export "payload" (type $value (eq $outer)))
        (type $future (future $value))
        (export "pending" (type $pending (eq $future)))
        (export "make" (func (param "value" $value) (result $pending)))))"#;
    let producer = wat::parse_str(format!(
        r#"(component {foreign}
        (import "test:shared/foreign" (instance $foreign (type $foreign)))
        (import "test:shared/other" (instance (type $foreign)))
        (alias export $foreign "payload" (type $payload))
        {api}
        (import "api-input" (instance $api (type $api)))
        (import "start" (func $kick async))
        (export "test:shared/api" (instance $api))
        (export "actor" (func $kick))
        (export "signal" (func $kick)))"#
    ))
    .unwrap();
    let facade = |wrong: bool| {
        wat::parse_str(format!(
            r#"(component {foreign}
        (import "test:shared/foreign" (instance $foreign (type $foreign)))
        (import "test:shared/other" (instance $other (type $foreign)))
        (alias export ${which} "payload" (type $payload))
        {api}
        (import "source" (instance $source (type $api)))
        (import "kick" (func async))
        (alias export $source "make" (func $make))
        (instance $forward (export "forward" (func $make)))
        (export "forwarder" (instance $forward)))"#,
            which = if wrong { "other" } else { "foreign" }
        ))
        .unwrap()
    };
    let plan = serde_json::to_vec(&serde_json::json!({
        "schema_version":1, "actor":"actor", "signal":"signal", "forward_interface":"forwarder",
        "imports":[{"local":"source","original":"test:shared/api"},{"local":"kick","original":"actor"}],
        "forwards":[{"interface":"test:shared/api","function":"make","export":"forward"}]
    })).unwrap();
    Validator::new_with_features(WasmFeatures::all())
        .validate_all(&producer)
        .unwrap();
    let bytes = compose_experimental_guest(&producer, &facade(false), &plan).unwrap();
    let mut types = Types::default();
    let package = Package::from_bytes("test:shared-result", None, bytes, &mut types).unwrap();
    assert_eq!(types[package.ty()].exports.len(), 1);
    let error = compose_experimental_guest(&producer, &facade(true), &plan).unwrap_err();
    assert!(
        format!("{error:#}").to_lowercase().contains("resource"),
        "{error:#}"
    );
}

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
    let Some(ValueType::Own(result)) = types[make].result else {
        panic!("owned resource result")
    };
    assert_eq!(
        types.resolve_resource(result),
        types.resolve_resource(resource)
    );
    let ItemKind::Func(later) = exports["later"] else {
        panic!("async function")
    };
    assert!(types[later].is_async);
}
