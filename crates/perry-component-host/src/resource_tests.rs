use super::*;
use serde_json::json;
use std::sync::Mutex;

pub(crate) const WORLD: &str = r#"
package perry:resources@1.0.0;
interface observe { dropped: func(value: u32); }
interface counters {
  resource counter { constructor(value: u32); read: func() -> u32; }
  consume: func(value: own<counter>);
  consume-two: func(first: own<counter>, second: own<counter>);
}
world plugin { import observe; export counters; }
"#;

pub(crate) const COMPONENT: &str = r#"
(component
  (import "perry:resources/observe@1.0.0" (instance $observe
    (export "dropped" (func (param "value" u32)))))
  (alias export $observe "dropped" (func $dropped))
  (component $implementation
  (import "dropped" (func $dropped (param "value" u32)))
  (core func $dropped (canon lower (func $dropped)))
  (type $counter-rep (resource (rep i32) (dtor (core func $dropped))))
  (export $counter "counter" (type $counter-rep))
  (core func $new (canon resource.new $counter))
  (core func $rep (canon resource.rep $counter))
  (core func $drop (canon resource.drop $counter))
  (core instance $ops
    (export "new" (func $new))
    (export "rep" (func $rep))
    (export "drop" (func $drop)))
  (core module $code
    (import "ops" "new" (func $new (param i32) (result i32)))
    (import "ops" "rep" (func $rep (param i32) (result i32)))
    (import "ops" "drop" (func $drop (param i32)))
    (func (export "new") (param i32) (result i32) (call $new (local.get 0)))
    ;; A borrow lowered into the component that defines this resource is its
    ;; representation already, rather than an own-handle table index.
    (func (export "read") (param i32) (result i32) (local.get 0))
    (func (export "consume") (param i32) (call $drop (local.get 0)))
    (func (export "consume-two") (param i32 i32)
      (call $drop (local.get 0)) (call $drop (local.get 1))))
  (core instance $code (instantiate $code (with "ops" (instance $ops))))
  (alias core export $code "new" (core func $new-counter))
  (alias core export $code "read" (core func $read))
  (alias core export $code "consume" (core func $consume))
  (alias core export $code "consume-two" (core func $consume-two))
  (func (export "[constructor]counter") (param "value" u32) (result (own $counter)) (canon lift (core func $new-counter)))
  (func (export "[method]counter.read") (param "self" (borrow $counter)) (result u32) (canon lift (core func $read)))
  (func (export "consume") (param "value" (own $counter)) (canon lift (core func $consume)))
  (func (export "consume-two") (param "first" (own $counter)) (param "second" (own $counter)) (canon lift (core func $consume-two))))
  (instance $exports (instantiate $implementation (with "dropped" (func $dropped))))
  (export "perry:resources/counters@1.0.0" (instance $exports)))
"#;

const INTERFACE: &str = "perry:resources/counters@1.0.0";

fn host(drops: Arc<Mutex<Vec<Value>>>) -> Host {
    let observer: Import = Arc::new(move |args| {
        drops.lock().unwrap().extend_from_slice(args);
        Ok(None)
    });
    host_with_observer(observer)
}

fn host_with_observer(observer: Import) -> Host {
    let imports = BTreeMap::from([(
        "perry:resources/observe@1.0.0".into(),
        BTreeMap::from([("dropped".into(), observer)]),
    )]);
    Host::new(
        &wat::parse_str(COMPONENT).unwrap(),
        &perry_wit::parse(WORLD, None).unwrap(),
        &imports,
        &Options::default(),
    )
    .unwrap()
}

#[test]
fn destructor_trap_releases_the_store_without_reentering_the_poisoned_component() {
    let drops: Arc<Mutex<Vec<Value>>> = Arc::default();
    let observed = drops.clone();
    let mut host = host_with_observer(Arc::new(move |args| {
        observed.lock().unwrap().extend_from_slice(args);
        if args == [json!(42)] {
            bail!("deliberate destructor failure");
        }
        Ok(None)
    }));
    for value in [42, 7] {
        host.call(INTERFACE, "[constructor]counter", &[json!(value)])
            .unwrap();
    }
    let error = host.dispose().unwrap_err();
    assert!(format!("{error:#}").contains("deliberate destructor failure"));
    // Canonical ABI traps poison the component: executing its next guest
    // destructor is forbidden. Teardown must still free the Store and its
    // captured callbacks, report the original failure, and remain idempotent.
    assert_eq!(*drops.lock().unwrap(), vec![json!(42)]);
    assert!(host.live.is_none());
    assert_eq!(
        Arc::strong_count(&drops),
        1,
        "the store retained a callback"
    );
    host.dispose().unwrap();
    drop(host);
    assert_eq!(*drops.lock().unwrap(), vec![json!(42)]);
}

#[test]
fn resource_ownership_borrows_transfer_and_exactly_once_disposal() {
    let drops: Arc<Mutex<Vec<Value>>> = Arc::default();
    let mut host = host(drops.clone());
    let resource = host
        .call(INTERFACE, "[constructor]counter", &[json!(42)])
        .unwrap()
        .unwrap();
    let mut borrowed = resource.clone();
    borrowed["owned"] = json!(false);
    assert_eq!(
        host.call(INTERFACE, "[method]counter.read", &[borrowed.clone()])
            .unwrap(),
        Some(json!(42))
    );
    assert!(host.call(INTERFACE, "consume", &[borrowed]).is_err());
    assert!(host
        .call(
            INTERFACE,
            "consume-two",
            &[resource.clone(), resource.clone()]
        )
        .is_err());
    assert!(drops.lock().unwrap().is_empty());
    host.call(INTERFACE, "consume", &[resource.clone()])
        .unwrap();
    assert_eq!(*drops.lock().unwrap(), vec![json!(42)]);
    assert!(host
        .call(INTERFACE, "[method]counter.read", &[resource])
        .is_err());
    let resource = host
        .call(INTERFACE, "[constructor]counter", &[json!(7)])
        .unwrap()
        .unwrap();
    host.drop_resource(resource["$resource"].as_str().unwrap())
        .unwrap();
    assert!(host
        .drop_resource(resource["$resource"].as_str().unwrap())
        .is_err());
    host.call(INTERFACE, "[constructor]counter", &[json!(9)])
        .unwrap();
    host.dispose().unwrap();
    host.dispose().unwrap();
    drop(host);
    assert_eq!(*drops.lock().unwrap(), vec![json!(42), json!(7), json!(9)]);
}

#[test]
fn resource_from_another_instance_cannot_select_a_local_handle() {
    let mut first = host(Arc::default());
    let mut second = host(Arc::default());
    let foreign = first
        .call(INTERFACE, "[constructor]counter", &[json!(1)])
        .unwrap()
        .unwrap();
    let local = second
        .call(INTERFACE, "[constructor]counter", &[json!(2)])
        .unwrap()
        .unwrap();
    assert!(second.call(INTERFACE, "consume", &[foreign]).is_err());
    assert_eq!(
        second
            .call(INTERFACE, "[method]counter.read", &[local])
            .unwrap(),
        Some(json!(2))
    );
}

#[test]
fn deferred_guest_cleanup_ignores_transferred_tokens_and_drops_live_owners_once() {
    use crate::cleanup::CollectedResource;
    let drops: Arc<Mutex<Vec<Value>>> = Arc::default();
    let mut host = host(drops.clone());
    for value in [31, 32] {
        let resource = host
            .call(INTERFACE, "[constructor]counter", &[json!(value)])
            .unwrap()
            .unwrap();
        let token = resource["$resource"].as_str().unwrap().to_string();
        if value == 31 {
            host.call(INTERFACE, "consume", &[resource]).unwrap();
        }
        for attempt in 0..2 {
            let mut released = false;
            host.collect_resource(
                CollectedResource::Guest {
                    token: token.clone(),
                },
                &mut released,
            )
            .unwrap();
            assert_eq!(released, value == 32 && attempt == 0);
        }
    }
    host.dispose().unwrap();
    assert_eq!(*drops.lock().unwrap(), [json!(31), json!(32)]);
}

#[test]
fn deferred_destructor_failure_retires_ownership_before_reporting_the_error() {
    use crate::cleanup::CollectedResource;
    let drops: Arc<Mutex<Vec<Value>>> = Arc::default();
    let observed = drops.clone();
    let mut host = host_with_observer(Arc::new(move |args| {
        observed.lock().unwrap().extend_from_slice(args);
        bail!("deferred destructor failed")
    }));
    let resource = host
        .call(INTERFACE, "[constructor]counter", &[json!(7)])
        .unwrap()
        .unwrap();
    let token = resource["$resource"].as_str().unwrap().to_string();
    let mut released = false;
    assert!(host
        .collect_resource(
            CollectedResource::Guest {
                token: token.clone()
            },
            &mut released
        )
        .is_err());
    assert!(released);
    released = false;
    host.collect_resource(CollectedResource::Guest { token }, &mut released)
        .unwrap();
    assert!(!released);
    host.dispose().unwrap();
    assert_eq!(*drops.lock().unwrap(), [json!(7)]);
}
