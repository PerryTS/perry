//! Architecture proof for a guest-owned writer escaping a synchronous factory.
//! The internal async task uses the same component instance and handles. This
//! does not implement Perry's value adapters or qualify their JS/GC ownership.
use super::integration_tests::result;
use crate::{AsyncImports, Host, Imports, Options};
use serde_json::json;
use std::sync::Arc;

#[test]
fn internal_callback_reactor_drives_a_sync_factory_stream_without_another_guest_call() {
    let world = perry_wit::parse(
        "package test:sync-reactor; world plugin { export make: func() -> stream<u8>; export finished: func() -> u32; }",
        None,
    ).unwrap();
    let bytes = wat::parse_str(include_str!("sync-reactor.component.wat")).unwrap();
    let host = Host::new_for_driver(
        &bytes,
        &world,
        &Imports::new(),
        &AsyncImports::new(),
        &Options::default(),
        Arc::new(|_| Ok(Vec::new())),
    )
    .unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let value = result(&mut driver, &mut client.call("", "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    // Each read must unblock a distinct write. No exported guest function is
    // invoked between receiving the stream and observing its final EOF.
    for byte in [65, 66] {
        let chunk = result(
            &mut driver,
            &mut client.read_async_value(&value, Some(1)).unwrap(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(chunk, json!({"done":false,"value":[byte]}));
    }
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&value, Some(1)).unwrap()
        )
        .unwrap(),
        Some(json!({"done":true}))
    );
    assert_eq!(
        result(&mut driver, &mut client.call("", "finished", &[]).unwrap()).unwrap(),
        Some(json!(1))
    );
}
