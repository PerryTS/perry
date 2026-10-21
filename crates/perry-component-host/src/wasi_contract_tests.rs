use crate::{Host, Imports, Options, WasiOptions};

const WORLD: &str = r#"
package wasi:clocks@0.2.12;
interface monotonic-clock {
    now: func() -> u64;
    resolution: func() -> u64;
}
interface unused-clock { unused: func(); }
world plugin {
    import monotonic-clock;
    import unused-clock;
    export read: func() -> u64;
}
"#;

const COMPONENT: &str = r#"(component
    (import "wasi:clocks/monotonic-clock@0.2.12" (instance $clock
        (export "now" (func (result u64)))))
    (alias export $clock "now" (func $now))
    (core func $lower (canon lower (func $now)))
    (core module $code
        (import "env" "now" (func $now (result i64)))
        (func (export "read") (result i64) (call $now)))
    (core instance $env (export "now" (func $lower)))
    (core instance $code (instantiate $code (with "env" (instance $env))))
    (alias core export $code "read" (core func $read))
    (func (export "read") (result u64) (canon lift (core func $read)))
)"#;

fn options() -> Options {
    Options {
        wasi: Some(WasiOptions::default()),
        ..Default::default()
    }
}

fn load_error(component: &str, world: &str, options: &Options) -> String {
    let bytes = wat::parse_str(component).unwrap();
    let world = perry_wit::parse(world, None).unwrap();
    let error = Host::new(&bytes, &world, &Imports::new(), options)
        .err()
        .expect("the incompatible contract must fail before execution");
    format!("{error:#}")
}

#[test]
fn pruned_provider_members_and_whole_interfaces_do_not_require_unused_imports() {
    let bytes = wat::parse_str(COMPONENT).unwrap();
    let world = perry_wit::parse(WORLD, None).unwrap();
    let mut host = Host::new(&bytes, &world, &Imports::new(), &options()).unwrap();
    let result = host.call("", "read", &[]).unwrap().unwrap();
    assert!(result["$bigint"].as_str().unwrap().parse::<u64>().unwrap() > 0);
    host.dispose().unwrap();
    assert!(
        load_error(COMPONENT, WORLD, &Options::default()).contains("missing function resolution")
    );
}

#[test]
fn a_different_provider_version_is_not_mistaken_for_a_pruned_interface() {
    let component = COMPONENT.replace("@0.2.12", "@0.2.11");
    let error = load_error(&component, WORLD, &options());
    assert!(
        error.contains("component imports do not match WIT"),
        "{error}"
    );
    assert!(error.contains("different version"), "{error}");
}

#[test]
fn present_provider_function_types_are_checked_before_component_initialization() {
    let component = COMPONENT.replace(
        "(func (export \"read\") (result i64) (call $now))",
        "(func (export \"read\") (result i64) (call $now)) (func $init unreachable) (start $init)",
    );
    let world = WORLD.replace("now: func() -> u64", "now: func() -> u32");
    let error = load_error(&component, &world, &options());
    assert!(
        error.contains("component imports do not match WIT"),
        "{error}"
    );
    assert!(error.contains("WIT type mismatch"), "{error}");
    assert!(
        !error.contains("unreachable"),
        "validation must precede initialization: {error}"
    );
}

#[test]
fn application_imports_and_exports_still_require_the_declared_functions() {
    let component = COMPONENT.replace("wasi:clocks", "perry:clocks");
    let world = WORLD.replace("wasi:clocks", "perry:clocks");
    let error = load_error(&component, &world, &options());
    assert!(error.contains("missing function resolution"), "{error}");
    let world = WORLD.replace(
        "export read: func() -> u64;",
        "export read: func() -> u64; export missing: func();",
    );
    let error = load_error(COMPONENT, &world, &options());
    assert!(
        error.contains("component exports do not match WIT"),
        "{error}"
    );
    assert!(error.contains("missing function missing"), "{error}");
}
