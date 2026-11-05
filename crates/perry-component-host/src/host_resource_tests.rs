use super::*;
use serde_json::json;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Mutex,
};

pub(crate) const WORLD: &str = r#"
package perry:host-resources@1.0.0;
interface tokens {
  resource token { constructor(); read: func() -> u32; }
  resource other;
  consume: func(value: own<token>);
}
world plugin {
  import tokens;
  use tokens.{token};
  export make: func() -> own<token>;
  export echo: func(value: own<token>) -> own<token>;
  export read: func(value: borrow<token>) -> u32;
  export consume: func(value: own<token>);
  export consume-imported: func(value: own<token>);
  export consume-two: func(first: own<token>, second: own<token>);
  export checked: func(value: own<token>, amount: u8);
  export mixed: func(owner: own<token>, borrowed: borrow<token>);
  export keep: func(value: own<token>);
  export fail: func(value: own<token>);
}
"#;

pub(crate) const COMPONENT: &str = r#"
(component
  (import "perry:host-resources/tokens@1.0.0" (instance $tokens
    (export "token" (type $token (sub resource)))
    (export "other" (type (sub resource)))
    (export "[constructor]token" (func (result (own $token))))
    (export "[method]token.read" (func (param "self" (borrow $token)) (result u32)))
    (export "consume" (func (param "value" (own $token))))))
  (alias export $tokens "token" (type $token))
  (alias export $tokens "[constructor]token" (func $make))
  (alias export $tokens "[method]token.read" (func $read))
  (alias export $tokens "consume" (func $consume))
  (core func $make (canon lower (func $make)))
  (core func $read (canon lower (func $read)))
  (core func $consume (canon lower (func $consume)))
  (core func $drop (canon resource.drop $token))
  (core instance $ops (export "make" (func $make)) (export "read" (func $read)) (export "drop" (func $drop)) (export "consume" (func $consume)))
  (core module $code
    (import "ops" "make" (func $make (result i32)))
    (import "ops" "read" (func $read (param i32) (result i32)))
    (import "ops" "drop" (func $drop (param i32)))
    (import "ops" "consume" (func $consume (param i32)))
    (global $held (mut i32) (i32.const 0))
    (func (export "make") (result i32) (call $make))
    (func (export "echo") (param i32) (result i32) (local.get 0))
    ;; Unlike a resource defined here, an imported borrow is a table handle.
    ;; Release it after forwarding the borrow, while preserving the read value.
    (func (export "read") (param i32) (result i32)
      (call $read (local.get 0)) (call $drop (local.get 0)))
    (func (export "consume") (param i32) (call $drop (local.get 0)))
    (func (export "consume-imported") (param i32) (call $consume (local.get 0)))
    (func (export "consume-two") (param i32 i32) (call $drop (local.get 0)) (call $drop (local.get 1)))
    (func (export "checked") (param i32 i32) (call $drop (local.get 0)))
    (func (export "mixed") (param i32 i32)
      (call $drop (local.get 0)) (call $drop (local.get 1)))
    (func (export "keep") (param i32) (global.set $held (local.get 0)))
    (func (export "fail") (param i32) unreachable)
    ;; INITIALIZER
  )
  (core instance $code (instantiate $code (with "ops" (instance $ops))))
  (func (export "make") (result (own $token)) (canon lift (core func $code "make")))
  (func (export "echo") (param "value" (own $token)) (result (own $token)) (canon lift (core func $code "echo")))
  (func (export "read") (param "value" (borrow $token)) (result u32) (canon lift (core func $code "read")))
  (func (export "consume") (param "value" (own $token)) (canon lift (core func $code "consume")))
  (func (export "consume-imported") (param "value" (own $token)) (canon lift (core func $code "consume-imported")))
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
    let pending = Arc::new(Mutex::new(Vec::new()));
    let pending_import = pending.clone();
    let consume: Import = Arc::new(move |args| {
        ensure!(args[0]["owned"] == true, "expected owned callback argument");
        pending_import.lock().unwrap().push(ResourceRef {
            token: args[0]["$hostResource"].as_str().unwrap().into(),
            type_key: args[0]["type"].as_str().unwrap().into(),
            generation: args[0]["generation"].as_u64().unwrap() as u32,
        });
        ensure!(
            args[0]["$hostResource"] != "bad-import",
            "deliberate import failure"
        );
        Ok(None)
    });
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
            ("consume".into(), consume),
        ]),
    )]);
    Host::new_with_resource_hook(
        &wat::parse_str(component).unwrap(),
        &perry_wit::parse(WORLD, None).unwrap(),
        &imports,
        &Options::default(),
        Arc::new(move |event| {
            if matches!(event, ResourceEvent::Drain) {
                return Ok(std::mem::take(&mut *pending.lock().unwrap()));
            }
            let fail = matches!(&event, ResourceEvent::Drop { resource } if trap_drop && resource.token == "bad");
            events.lock().unwrap().push(event);
            ensure!(!fail, "deliberate host payload destructor failure");
            Ok(Vec::new())
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
fn deferred_host_cleanup_checks_generation_and_current_ownership() {
    use crate::cleanup::CollectedResource;
    let events: Arc<Mutex<Vec<ResourceEvent>>> = Arc::default();
    let mut host = host(COMPONENT, events.clone(), false).unwrap();
    let original = fresh("collected");
    let returned = host
        .call("", "echo", std::slice::from_ref(&original))
        .unwrap()
        .unwrap();
    let reference = |value: &Value| ResourceRef {
        token: value["$hostResource"].as_str().unwrap().into(),
        type_key: value["type"].as_str().unwrap().into(),
        generation: value["generation"].as_u64().unwrap() as u32,
    };
    let mut released = false;
    host.collect_resource(
        CollectedResource::Host {
            resource: reference(&original),
        },
        &mut released,
    )
    .unwrap();
    assert!(!released);
    assert!(drops(&events).is_empty());
    let mut borrowed = returned.clone();
    borrowed["owned"] = json!(false);
    assert_eq!(host.call("", "read", &[borrowed]).unwrap(), Some(json!(42)));
    host.collect_resource(
        CollectedResource::Host {
            resource: reference(&returned),
        },
        &mut released,
    )
    .unwrap();
    assert!(released);
    released = false;
    host.collect_resource(
        CollectedResource::Host {
            resource: reference(&returned),
        },
        &mut released,
    )
    .unwrap();
    assert!(!released);

    let kept = fresh("guest-held");
    host.call("", "keep", std::slice::from_ref(&kept)).unwrap();
    host.collect_resource(
        CollectedResource::Host {
            resource: reference(&kept),
        },
        &mut released,
    )
    .unwrap();
    assert!(!released);
    assert_eq!(drops(&events), ["collected"]);
    host.dispose().unwrap();
    assert_eq!(drops(&events), ["collected", "guest-held"]);
}

#[test]
fn owned_import_arguments_can_be_disposed_without_store_reentry_even_on_failure() {
    for token in ["imported", "bad-import"] {
        let events: Arc<Mutex<Vec<ResourceEvent>>> = Arc::default();
        let mut host = host(COMPONENT, events.clone(), false).unwrap();
        let result = host.call("", "consume-imported", &[fresh(token)]);
        assert_eq!(result.is_err(), token == "bad-import");
        assert_eq!(drops(&events), [token]);
        host.dispose().unwrap();
        assert_eq!(drops(&events), [token]);
    }
}

#[test]
fn host_resources_round_trip_borrow_and_drop_with_distinct_generations() {
    let events: Arc<Mutex<Vec<ResourceEvent>>> = Arc::default();
    let mut host = host(COMPONENT, events.clone(), false).unwrap();
    let original = fresh("local");
    let returned = host
        .call("", "echo", std::slice::from_ref(&original))
        .unwrap()
        .unwrap();
    assert_eq!(returned["$hostResource"], "local");
    assert_eq!(returned["generation"], 2);
    assert!(host.call("", "echo", &[original]).is_err());
    for _ in 0..3 {
        assert_eq!(
            host.call("", "read", std::slice::from_ref(&returned))
                .unwrap(),
            Some(json!(42))
        );
    }
    host.call("", "consume", std::slice::from_ref(&returned))
        .unwrap();
    assert!(host.call("", "consume", &[returned]).is_err());
    assert_eq!(drops(&events), ["local"]);
    let made = host.call("", "make", &[]).unwrap().unwrap();
    assert_eq!(made["generation"], 2);
    host.drop_resource(made["$hostResource"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        drops(&events),
        ["local", "made-0"],
        "explicit disposal must not wait for instance teardown"
    );
    host.dispose().unwrap();
    assert_eq!(drops(&events), ["local", "made-0"]);
}

#[test]
fn explicit_host_drop_preserves_a_destructor_failure_and_never_retries_it() {
    let events: Arc<Mutex<Vec<ResourceEvent>>> = Arc::default();
    let mut host = host(COMPONENT, events.clone(), true).unwrap();
    host.call("", "echo", &[fresh("bad")]).unwrap();
    let mut released = false;
    let error = host
        .drop_resource_tracked("bad", &mut released)
        .unwrap_err();
    assert!(released);
    assert!(format!("{error:#}").contains("deliberate host payload destructor failure"));
    assert_eq!(drops(&events), ["bad"]);
    released = false;
    assert!(host.drop_resource_tracked("bad", &mut released).is_err());
    assert!(
        !released,
        "a rejected second drop must not claim to release ownership"
    );
    host.dispose().unwrap();
    assert_eq!(drops(&events), ["bad"]);
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
    // Distinct owners can be transferred and borrowed in the same call.
    let borrowed = host
        .call("", "echo", &[fresh("borrowed")])
        .unwrap()
        .unwrap();
    host.call("", "mixed", &[fresh("transferred"), borrowed.clone()])
        .unwrap();
    assert_eq!(drops(&events), ["rollback", "transferred"]);
    assert_eq!(host.call("", "read", &[borrowed]).unwrap(), Some(json!(42)));
    host.drop_resource("borrowed").unwrap();
    assert_eq!(drops(&events), ["rollback", "transferred", "borrowed"]);
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
