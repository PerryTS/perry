use super::*;
use serde_json::json;
use std::sync::Mutex;
use wasmtime::component::Type;

pub(super) const WORLD: &str = r#"
package perry:test@1.0.0;
interface logging { log: func(message: string); }
interface transform {
  record request { text: string, bytes: list<u8> }
  echo: func(input: request) -> request;
  returned: func() -> u32;
  grow: func() -> u32;
  spin: func();
}
world plugin { import logging; export transform; }
"#;

pub(super) const COMPONENT: &str = r#"
(component
  (import "perry:test/logging@1.0.0" (instance $logging
    (export "log" (func (param "message" string)))))
  (alias export $logging "log" (func $log))
  (core module $memory
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 128))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $ptr i32)
      (global.set $heap (i32.and
        (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
        (i32.sub (i32.const 0) (local.get 2))))
      (local.set $ptr (global.get $heap))
      (global.set $heap (i32.add (global.get $heap) (local.get 3)))
      (local.get $ptr)))
  (core instance $mem (instantiate $memory))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $log (canon lower (func $log) (memory $memory)))
  (core instance $env
    (export "memory" (memory $memory))
    (export "log" (func $log)))
  (core module $code
    (import "env" "memory" (memory 1))
    (import "env" "log" (func $log (param i32 i32)))
    (global $returned (mut i32) (i32.const 0))
    (func (export "echo") (param i32 i32 i32 i32) (result i32)
      (call $log (local.get 0) (local.get 1))
      (i32.store (i32.const 32) (local.get 0))
      (i32.store (i32.const 36) (local.get 1))
      (i32.store (i32.const 40) (local.get 2))
      (i32.store (i32.const 44) (local.get 3))
      (i32.const 32))
    (func (export "post") (param i32)
      (global.set $returned (i32.add (global.get $returned) (i32.const 1))))
    (func (export "returned") (result i32) (global.get $returned))
    (func (export "grow") (result i32) (memory.grow (i32.const 1)))
    (func (export "spin") (loop $forever (br $forever))))
  (core instance $code (instantiate $code (with "env" (instance $env))))
  (alias core export $code "echo" (core func $echo))
  (alias core export $code "post" (core func $post))
  (alias core export $code "returned" (core func $returned))
  (alias core export $code "grow" (core func $grow))
  (alias core export $code "spin" (core func $spin))
  (type $request (record (field "text" string) (field "bytes" (list u8))))
  (func $echo (param "input" $request) (result $request)
    (canon lift (core func $echo) (memory $memory) (realloc $realloc) (post-return $post)))
  (func $returned (result u32) (canon lift (core func $returned)))
  (func $grow (result u32) (canon lift (core func $grow)))
  (func $spin (canon lift (core func $spin)))
  (instance $exports
    (export "request" (type $request))
    (export "echo" (func $echo))
    (export "returned" (func $returned))
    (export "grow" (func $grow))
    (export "spin" (func $spin)))
  (export "perry:test/transform@1.0.0" (instance $exports)))
"#;

fn imports(log: Arc<Mutex<Vec<Value>>>) -> Imports {
    let callback: Import = Arc::new(move |args| {
        log.lock().unwrap().extend_from_slice(args);
        Ok(None)
    });
    BTreeMap::from([(
        "perry:test/logging@1.0.0".into(),
        BTreeMap::from([("log".into(), callback)]),
    )])
}

fn host(options: Options) -> Host {
    Host::new(
        &wat::parse_str(COMPONENT).unwrap(),
        &perry_wit::parse(WORLD, None).unwrap(),
        &imports(Arc::default()),
        &options,
    )
    .unwrap()
}

#[test]
fn records_strings_bytes_imports_and_automatic_post_return() {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let mut host = Host::new(
        &wat::parse_str(COMPONENT).unwrap(),
        &perry_wit::parse(WORLD, None).unwrap(),
        &imports(logs.clone()),
        &Options::default(),
    )
    .unwrap();
    for n in 0..20 {
        let input = json!({"text":format!("héllo 🌍 {n}"),"bytes":[0,1,127,255]});
        assert_eq!(
            host.call("perry:test/transform@1.0.0", "echo", &[input.clone()])
                .unwrap(),
            Some(input)
        );
        assert_eq!(
            host.call("perry:test/transform@1.0.0", "returned", &[])
                .unwrap(),
            Some(json!(n + 1))
        );
    }
    assert_eq!(logs.lock().unwrap().len(), 20);
    host.dispose().unwrap();
    host.dispose().unwrap();
    assert!(host
        .call("perry:test/transform@1.0.0", "returned", &[])
        .unwrap_err()
        .to_string()
        .contains("disposed"));
}

#[test]
fn interface_version_signature_and_missing_import_fail_before_calls() {
    let bytes = wat::parse_str(COMPONENT).unwrap();
    let world = perry_wit::parse(WORLD, None).unwrap();
    let logs = Arc::new(Mutex::new(Vec::new()));
    assert!(Host::new(&bytes, &world, &Imports::new(), &Options::default()).is_err());
    let wrong_version = perry_wit::parse(&WORLD.replace("@1.0.0", "@2.0.0"), None).unwrap();
    assert!(Host::new(
        &bytes,
        &wrong_version,
        &imports(logs.clone()),
        &Options::default()
    )
    .is_err());
    let wrong_type = perry_wit::parse(
        &WORLD.replace("returned: func() -> u32", "returned: func() -> u64"),
        None,
    )
    .unwrap();
    assert!(Host::new(
        &bytes,
        &wrong_type,
        &imports(logs.clone()),
        &Options::default()
    )
    .is_err());
    assert!(logs.lock().unwrap().is_empty());
}

#[test]
fn invalid_values_do_not_enter_guest_and_next_call_works() {
    let mut host = host(Options::default());
    assert!(host
        .call(
            "perry:test/transform@1.0.0",
            "echo",
            &[json!({"text":"bad","bytes":[256]})]
        )
        .is_err());
    assert_eq!(
        host.call("perry:test/transform@1.0.0", "returned", &[])
            .unwrap(),
        Some(json!(0))
    );
}

#[test]
fn failed_initialization_releases_imports_and_does_not_affect_another_instance() {
    let source = COMPONENT.replace(
        "(memory (export \"memory\") 1)",
        "(memory (export \"memory\") 1) (func $init unreachable) (start $init)",
    );
    let bytes = wat::parse_str(&source).unwrap();
    let world = perry_wit::parse(WORLD, None).unwrap();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let callbacks = imports(logs.clone());
    let callback = &callbacks["perry:test/logging@1.0.0"]["log"];
    let owners = Arc::strong_count(callback);
    for _ in 0..3 {
        let error = Host::new(&bytes, &world, &callbacks, &Options::default())
            .err()
            .expect("the core start function must trap");
        assert!(format!("{error:#}").contains("unreachable"));
        assert_eq!(
            Arc::strong_count(callback),
            owners,
            "failed initialization must release its linker/store callbacks"
        );
    }
    assert!(logs.lock().unwrap().is_empty());
    assert_eq!(
        host(Options::default())
            .call("perry:test/transform@1.0.0", "returned", &[])
            .unwrap(),
        Some(json!(0))
    );
}

#[test]
fn memory_and_fuel_limits_stop_guest_execution() {
    let mut host = host(Options {
        memory_bytes: 65536,
        fuel: 1000,
        ..Options::default()
    });
    assert!(host
        .call("perry:test/transform@1.0.0", "grow", &[])
        .is_err());
    let mut host = self::host(Options {
        fuel: 1000,
        ..Options::default()
    });
    let error = host
        .call("perry:test/transform@1.0.0", "spin", &[])
        .unwrap_err();
    assert!(format!("{error:#}").contains("fuel"));
}

#[test]
fn interrupt_stops_only_its_component() {
    let mut spinning = host(Options {
        fuel: u64::MAX,
        ..Options::default()
    });
    let interrupt = spinning.interrupt_handle();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(20));
        interrupt.interrupt();
    });
    let error = spinning
        .call("perry:test/transform@1.0.0", "spin", &[])
        .unwrap_err();
    thread.join().unwrap();
    assert!(format!("{error:#}").contains("interrupt"));
    assert_eq!(
        host(Options::default())
            .call("perry:test/transform@1.0.0", "returned", &[])
            .unwrap(),
        Some(json!(0))
    );
}

#[test]
fn exact_bigints_unicode_and_special_floats_round_trip() {
    let mut resources = resources::Resources::default();
    for (value, ty) in [
        (Val::U64(u64::MAX), Type::U64),
        (Val::S64(i64::MIN), Type::S64),
        (Val::Char('🌍'), Type::Char),
        (Val::Float64(f64::INFINITY), Type::Float64),
        (Val::Float64(-0.0), Type::Float64),
    ] {
        let encoded = wire::lift(&value, &mut resources).unwrap();
        let decoded = wire::lower(&encoded, &ty, &resources, &mut BTreeSet::new()).unwrap();
        assert_eq!(format!("{value:?}"), format!("{decoded:?}"));
    }
    assert!(wire::lower(
        &json!(9_007_199_254_740_991u64),
        &Type::U64,
        &resources,
        &mut BTreeSet::new()
    )
    .is_err());
    assert!(wire::lower(&json!("ab"), &Type::Char, &resources, &mut BTreeSet::new()).is_err());
}
