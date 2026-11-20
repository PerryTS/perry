use super::integration_tests::result;
use crate::{Host, Options};
use serde_json::{json, Value};
use std::collections::BTreeMap;
const NS: &str = "test:producer/data";
#[test]
fn future_transfer_checks_types_duplicates_and_aggregate_rollback_before_entering_guest() {
    let source="package test:producer; interface data { resource thing; make: async func() -> future<own<thing>>; drops: func() -> u32; consume: func(value: own<thing>); echo: func(value: future<own<thing>>) -> future<own<thing>>; wrong: func(value: future<u32>); pair: func(first: future<own<thing>>, second: future<own<thing>>); checked: func(value: future<own<thing>>, flag: bool) -> future<own<thing>>; unexpected: func() -> u32; } world plugin { export data; }";
    let world = perry_wit::parse(source, None).unwrap();
    let mut text = include_str!("producer.component.wat").to_string();
    let insertion = r#"
      (type $other (future u32))
      (core module $probe
        (global $unexpected (mut i32) (i32.const 0))
        (func (export "echo") (param i32) (result i32) (local.get 0))
        (func (export "checked") (param i32 i32) (result i32) (local.get 0))
        (func (export "wrong") (param i32) (global.set $unexpected (i32.const 1)) unreachable)
        (func (export "pair") (param i32 i32) (global.set $unexpected (i32.const 1)) unreachable)
        (func (export "unexpected") (result i32) (global.get $unexpected)))
      (core instance $probe (instantiate $probe))
      (func $echo (param "value" $future) (result $future) (canon lift (core func $probe "echo")))
      (func $checked (param "value" $future) (param "flag" bool) (result $future) (canon lift (core func $probe "checked")))
      (func $wrong (param "value" $other) (canon lift (core func $probe "wrong")))
      (func $pair (param "first" $future) (param "second" $future) (canon lift (core func $probe "pair")))
      (func $unexpected (result u32) (canon lift (core func $probe "unexpected")))
    "#;
    text = text.replace(
        "  (instance $data",
        &format!("{insertion}\n  (instance $data"),
    );
    text=text.replace("(instance $data (export \"thing\"","(instance $data (export \"echo\" (func $echo)) (export \"checked\" (func $checked)) (export \"wrong\" (func $wrong)) (export \"pair\" (func $pair)) (export \"unexpected\" (func $unexpected)) (export \"thing\"");
    let host = Host::new(
        &wat::parse_str(text).unwrap(),
        &world,
        &BTreeMap::new(),
        &Options::default(),
    )
    .unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let wrong = result(
        &mut driver,
        &mut client
            .call(NS, "wrong", std::slice::from_ref(&value))
            .unwrap(),
    )
    .unwrap_err();
    assert!(format!("{wrong:#}").contains("async payload type mismatch"));
    let duplicate = result(
        &mut driver,
        &mut client
            .call(NS, "pair", &[value.clone(), value.clone()])
            .unwrap(),
    )
    .unwrap_err();
    assert!(format!("{duplicate:#}").contains("transferred more than once"));
    let invalid = result(
        &mut driver,
        &mut client
            .call(
                NS,
                "checked",
                &[value.clone(), Value::String("invalid".into())],
            )
            .unwrap(),
    )
    .unwrap_err();
    assert!(format!("{invalid:#}").contains("expected bool"));
    assert_eq!(
        result(
            &mut driver,
            &mut client.call(NS, "unexpected", &[]).unwrap()
        )
        .unwrap(),
        Some(json!(0))
    );
    let moved = result(
        &mut driver,
        &mut client
            .call(NS, "echo", std::slice::from_ref(&value))
            .unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_ne!(moved, value);
    assert!(result(
        &mut driver,
        &mut client.read_async_value(&value, None).unwrap()
    )
    .is_err());
    let owned = result(
        &mut driver,
        &mut client.read_async_value(&moved, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[owned]).unwrap()
    )
    .is_ok());
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(1))
    );
}
