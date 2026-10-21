use super::*;
use serde_json::json;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Mutex,
};

const WORLD: &str = r#"
package perry:host-resources@1.0.0;
interface tokens {
  resource token { constructor(); read: func() -> u32; }
  resource other;
}
world plugin {
  import tokens;
  use tokens.{token};
  export make: func() -> own<token>;
  export echo: func(value: own<token>) -> own<token>;
  export read: func(value: borrow<token>) -> u32;
  export consume: func(value: own<token>);
  export consume-two: func(first: own<token>, second: own<token>);
  export checked: func(value: own<token>, amount: u8);
  export mixed: func(owner: own<token>, borrowed: borrow<token>);
  export keep: func(value: own<token>);
  export fail: func(value: own<token>);
}
"#;

const COMPONENT: &str = r#"
(component
  (import "perry:host-resources/tokens@1.0.0" (instance $tokens
    (export "token" (type $token (sub resource)))
    (export "other" (type (sub resource)))
    (export "[constructor]token" (func (result (own $token))))
    (export "[method]token.read" (func (param "self" (borrow $token)) (result u32)))))
  (alias export $tokens "token" (type $token))
  (alias export $tokens "[constructor]token" (func $make))
  (alias export $tokens "[method]token.read" (func $read))
  (core func $make (canon lower (func $make)))
  (core func $read (canon lower (func $read)))
  (core func $drop (canon resource.drop $token))
  (core instance $ops (export "make" (func $make)) (export "read" (func $read)) (export "drop" (func $drop)))
  (core module $code
    (import "ops" "make" (func $make (result i32)))
    (import "ops" "read" (func $read (param i32) (result i32)))
    (import "ops" "drop" (func $drop (param i32)))
    (global $held (mut i32) (i32.const 0))
    (func (export "make") (result i32) (call $make))
    (func (export "echo") (param i32) (result i32) (local.get 0))
    (func (export "read") (param i32) (result i32) (call $read (local.get 0)))
    (func (export "consume") (param i32) (call $drop (local.get 0)))
    (func (export "consume-two") (param i32 i32) (call $drop (local.get 0)) (call $drop (local.get 1)))
    (func (export "checked") (param i32 i32) (call $drop (local.get 0)))
    (func (export "mixed") (param i32 i32) (call $drop (local.get 0)))
    (func (export "keep") (param i32) (global.set $held (local.get 0)))
    (func (export "fail") (param i32) unreachable)
    ;; INITIALIZER
  )
  (core instance $code (instantiate $code (with "ops" (instance $ops))))
  (func (export "make") (result (own $token)) (canon lift (core func $code "make")))
  (func (export "echo") (param "value" (own $token)) (result (own $token)) (canon lift (core func $code "echo")))
  (func (export "read") (param "value" (borrow $token)) (result u32) (canon lift (core func $code "read")))
  (func (export "consume") (param "value" (own $token)) (canon lift (core func $code "consume")))
  (func (export "consume-two") (param "first" (own $token)) (param "second" (own $token)) (canon lift (core func $code "consume-two")))
  (func (export "checked") (param "value" (own $token)) (param "amount" u8) (canon lift (core func $code "checked")))
  (func (export "mixed") (param "owner" (own $token)) (param "borrowed" (borrow $token)) (canon lift (core func $code "mixed")))
  (func (export "keep") (param "value" (own $token)) (canon lift (core func $code "keep")))
  (func (export "fail") (param "value" (own $token)) (canon lift (core func $code "fail"))))
"#;

fn fresh(token: &str) -> Value {
    let world = perry_wit::parse(WORLD, None).unwrap();
    let key = &world
        .imports
        .iter()
        .find(|i| i.name.ends_with("tokens@1.0.0"))
        .unwrap()
        .resources["token"];
    json!({"$hostResource":token,"type":key,"generation":1,"owned":true,"new":true})
}

fn host(component: &str, events: Arc<Mutex<Vec<ResourceEvent>>>, trap_drop: bool) -> Result<Host> {
    let next = AtomicU32::new(0);
    let make: Import = Arc::new(move |_| {
        Ok(Some(fresh(&format!(
            "made-{}",
            next.fetch_add(1, Ordering::Relaxed)
        ))))
    });
    let read: Import = Arc::new(|args| {
        ensure!(
            args.len() == 1 && args[0]["owned"] == false && args[0]["$hostResource"].is_string(),
            "expected borrowed host resource"
        );
        Ok(Some(json!(42)))
    });
    let imports = BTreeMap::from([(
        "perry:host-resources/tokens@1.0.0".into(),
        BTreeMap::from([
            ("[constructor]token".into(), make),
            ("[method]token.read".into(), read),
        ]),
    )]);
    Host::new_with_resource_hook(
        &wat::parse_str(component).unwrap(),
        &perry_wit::parse(WORLD, None).unwrap(),
        &imports,
        &Options::default(),
        Arc::new(move |event| {
            let fail = matches!(&event, ResourceEvent::Drop { resource } if trap_drop && resource.token == "bad");
            events.lock().unwrap().push(event);
            ensure!(!fail, "deliberate host payload destructor failure");
            Ok(())
        }),
    )
}

fn drops(events: &Arc<Mutex<Vec<ResourceEvent>>>) -> Vec<String> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            ResourceEvent::Drop { resource } => Some(resource.token.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn host_resources_round_trip_borrow_and_drop_with_distinct_generations() {
    let events: Arc<Mutex<Vec<ResourceEvent>>> = Arc::default();
    let mut host = host(COMPONENT, events.clone(), false).unwrap();
    let original = fresh("local");
    let returned = host.call("", "echo", &[original.clone()]).unwrap().unwrap();
    assert_eq!(returned["$hostResource"], "local");
    assert_eq!(returned["generation"], 2);
    assert!(host.call("", "echo", &[original]).is_err());
    assert_eq!(
        host.call("", "read", &[returned.clone()]).unwrap(),
        Some(json!(42))
    );
    host.call("", "consume", &[returned.clone()]).unwrap();
    assert!(host.call("", "consume", &[returned]).is_err());
    assert_eq!(drops(&events), ["local"]);
    let made = host.call("", "make", &[]).unwrap().unwrap();
    assert_eq!(made["generation"], 2);
    host.drop_resource(made["$hostResource"].as_str().unwrap())
        .unwrap();
    host.dispose().unwrap();
    assert_eq!(drops(&events), ["local", "made-0"]);
}

#[test]
fn failed_lowering_rolls_back_new_handles_and_preserves_owners() {
    let events: Arc<Mutex<Vec<ResourceEvent>>> = Arc::default();
    let mut host = host(COMPONENT, events.clone(), false).unwrap();
    let value = fresh("rollback");
    assert!(host
        .call("", "consume-two", &[value.clone(), value.clone()])
        .is_err());
    assert!(host
        .call("", "mixed", &[value.clone(), value.clone()])
        .is_err());
    assert!(host
        .call("", "checked", &[value.clone(), json!(999)])
        .is_err());
    assert!(events.lock().unwrap().is_empty());
    assert!(host
        .live
        .as_ref()
        .unwrap()
        .store
        .data()
        .host_resources
        .entries
        .is_empty());
    let value = host.call("", "echo", &[value]).unwrap().unwrap();
    host.call("", "consume", &[value]).unwrap();
    assert_eq!(drops(&events), ["rollback"]);
}

#[test]
fn teardown_releases_guest_held_payloads_even_after_a_trap_or_drop_failure() {
    let events: Arc<Mutex<Vec<ResourceEvent>>> = Arc::default();
    let mut host = host(COMPONENT, events.clone(), true).unwrap();
    host.call("", "keep", &[fresh("bad")]).unwrap();
    assert!(host.call("", "fail", &[fresh("good")]).is_err());
    let error = host.dispose().unwrap_err();
    assert!(format!("{error:#}").contains("deliberate host payload destructor failure"));
    host.dispose().unwrap();
    assert_eq!(drops(&events), ["bad", "good"]);
}

#[test]
fn initialization_failure_releases_resources_created_by_imports() {
    let events: Arc<Mutex<Vec<ResourceEvent>>> = Arc::default();
    let component = COMPONENT.replace(
        ";; INITIALIZER",
        "(func $init (drop (call $make)) unreachable) (start $init)",
    );
    assert!(host(&component, events.clone(), false).is_err());
    assert_eq!(drops(&events), ["made-0"]);
}
