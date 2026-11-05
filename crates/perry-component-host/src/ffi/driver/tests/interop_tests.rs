use super::*;
#[test]
fn ffi_driver_future_resource_payloads_keep_identity_through_private_disposal() {
    unsafe {
        let source="package test:producer; interface data { resource thing; make: async func() -> future<own<thing>>; drops: func() -> u32; consume: func(value: own<thing>); } world plugin { export data; }";
        let mut state = State::default();
        new(
            &mut state,
            source,
            include_str!("../../../async_values/producer.component.wat"),
        );
        for expected in 1..=3 {
            let future = completed(
                &mut state,
                json!({"op":"call","interface":"test:producer/data","function":"make","args":[]}),
            );
            let resource = completed(&mut state, json!({"op":"read","value":future}));
            assert_eq!(resource["owned"], true);
            completed(&mut state, json!({"op":"resource-drop","value":resource}));
            completed(
                &mut state,
                json!({"op":"resource-collect","request":{"kind":"guest","token":resource["$resource"]}}),
            );
            assert_eq!(
                completed(
                    &mut state,
                    json!({"op":"call","interface":"test:producer/data","function":"drops","args":[]})
                ),
                json!(expected)
            );
        }
        finish(&mut state);
    }
}
const WORLD:&str="package test:borrowed; interface items { resource item; } world plugin { import items; use items.{item}; import loan: async func(value: borrow<item>, ticket: u64); import ping: func(value: borrow<item>) -> u32; export run: async func(owner: own<item>, value: u64) -> u64; export inspect: func(value: borrow<item>) -> u32; }";
unsafe fn borrowed_imports(state: &mut State) {
    new(state, WORLD, &borrowed_text());
}
fn borrowed_text() -> String {
    let prefix = r#"(component
        (import "test:borrowed/items" (instance $items (export "item" (type $item (sub resource)))))
        (alias export $items "item" (type $item))
        (import "ping" (func $ping (param "value" (borrow $item)) (result u32)))
        (core func $ping (canon lower (func $ping)))
        (import "loan" (func $loan async (param "value" (borrow $item)) (param "ticket" u64)))
        (core func $drop-item (canon resource.drop $item))
        (core func $wait (canon lower (func $loan) async))
        (core module $inspect
            (import "host" "ping" (func $ping (param i32) (result i32)))
            (import "host" "drop" (func $drop (param i32)))
            (func (export "inspect") (param i32) (result i32) (call $ping (local.get 0)) (call $drop (local.get 0))))
        (core instance $inspect (instantiate $inspect (with "host" (instance (export "ping" (func $ping)) (export "drop" (func $drop-item))))))
        (func (export "inspect") (param "value" (borrow $item)) (result u32) (canon lift (core func $inspect "inspect")))
    "#;
    let original = include_str!("../../../async_values/task_context.component.wat");
    let start = original.find("  (core func $return").unwrap();
    let heap =
        include_str!("../../../async_values/heap.wat").replacen("(module", "(core module $heap", 1);
    let text=format!("{prefix}{}",&original[start..]).replace(";; HEAP",&heap)
        .replace("(core module $code","(core module $code (import \"host\" \"drop-item\" (func $drop-item (param i32)))")
        .replace("(import \"host\" \"wait\" (func $wait (param i64)","(import \"host\" \"wait\" (func $wait (param i32 i64)")
        .replace("(export \"get\" (func $get))","(export \"drop-item\" (func $drop-item)) (export \"get\" (func $get))")
        .replace("(param $value i64) (result i32)","(param $owner i32) (param $value i64) (result i32)")
        .replace("(func (export \"run\") async (param \"value\" u64)","(func (export \"run\") async (param \"owner\" (own $item)) (param \"value\" u64)")
        .replace("(call $alloc (i32.const 16))","(call $alloc (i32.const 24))")
        .replace("(local.set $status (call $wait (local.get $value)))","(i32.store offset=16 (local.get $frame) (local.get $owner)) (local.set $status (call $wait (i32.load offset=16 (local.get $frame)) (local.get $value)))")
        .replace("(call $return (i64.load (local.get $frame)))","(call $drop-item (i32.load offset=16 (local.get $frame))) (call $return (i64.load (local.get $frame)))");
    text
}
#[test]
fn ffi_driver_async_import_borrows_can_be_forwarded_until_their_individual_import_settles() {
    unsafe {
        let mut state = State::default();
        borrowed_imports(&mut state);
        let first = submit(state.host, owned_call(7)).unwrap();
        let second = submit(state.host, owned_call(8)).unwrap();
        assert_eq!(poll(&mut state, 10)["completed"], json!([]));
        let a = state.loans[&7].clone();
        let b = state.loans[&8].clone();
        assert_eq!(a["owned"], false);
        assert_eq!(b["owned"], false);
        assert_ne!(a["$hostResource"], b["$hostResource"]);
        for (resource, expected) in [(&a, 7), (&b, 8)] {
            assert_eq!(
                completed(
                    &mut state,
                    json!({"op":"call","interface":"","function":"inspect","args":[resource]})
                ),
                json!(expected)
            );
        }
        resolve(&mut state, 7, true);
        let done = poll(&mut state, 20);
        assert_eq!(done["completed"][0]["id"], first);
        assert_eq!(done["completed"][0]["ok"], true);
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            1
        );
        assert_eq!(
            completed(
                &mut state,
                json!({"op":"call","interface":"","function":"inspect","args":[b]})
            ),
            json!(8)
        );
        let stale = submit(
            state.host,
            json!({"op":"call","interface":"","function":"inspect","args":[a]}),
        )
        .unwrap_err();
        assert!(stale.to_string().contains("borrow capability expired"));
        let done = poll(&mut state, 30);
        assert_eq!(done["completed"], json!([]));
        assert_eq!(done["retired"], false);
        resolve(&mut state, 8, true);
        let done = poll(&mut state, 40);
        assert_eq!(done["completed"][0]["id"], second);
        assert_eq!(done["completed"][0]["ok"], true);
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            2
        );
        finish(&mut state);
    }
}

#[test]
fn ffi_driver_queued_forwarding_pins_borrows_before_the_import_settles() {
    unsafe {
        let mut state = State::default();
        borrowed_imports(&mut state);
        let owner = submit(state.host, owned_call(7)).unwrap();
        assert_eq!(poll(&mut state, 10)["completed"], json!([]));
        let loan = state.loans[&7].clone();
        let forwarded = submit(
            state.host,
            json!({"op":"call","interface":"","function":"inspect","args":[loan]}),
        )
        .unwrap();
        // The accepted command must retain its alias even if resolving the
        // original import wakes its owner before the mailbox is drained.
        resolve(&mut state, 7, true);
        let done = poll(&mut state, 20);
        let results = done["completed"].as_array().unwrap();
        assert_eq!(results.len(), 2, "{done}");
        for (id, value) in [(owner, json!({"$bigint":"7"})), (forwarded, json!(7))] {
            let result = results.iter().find(|r| r["id"] == id).unwrap();
            assert_eq!(result["ok"], true, "{result}");
            assert_eq!(result["value"], value);
        }
        assert_eq!(done["retired"], false);
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            1
        );
        finish(&mut state);
    }
}

#[test]
fn ffi_driver_callback_submission_preserves_the_borrow_after_an_immediate_import_result() {
    unsafe {
        let mut state = State {
            forward_during_loan: true,
            ..State::default()
        };
        borrowed_imports(&mut state);
        let owner = submit(state.host, owned_call(7)).unwrap();
        let mut results = Vec::new();
        for marker in 10..138 {
            let done = poll(&mut state, marker);
            assert_eq!(done["retired"], false, "{done}");
            results.extend(done["completed"].as_array().unwrap().iter().cloned());
            if results.len() == 2 {
                break;
            }
        }
        let forwarded = state.nested.as_ref().unwrap();
        assert_eq!(results.len(), 2);
        for id in [&owner, forwarded] {
            assert_eq!(
                results.iter().find(|r| r["id"] == *id).unwrap()["ok"],
                true,
                "{results:?}"
            );
        }
        assert_eq!(
            results.iter().find(|r| r["id"] == *forwarded).unwrap()["value"],
            7
        );
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            1
        );
        finish(&mut state);
    }
}

fn owned_call(ticket: u64) -> Value {
    let world = perry_wit::parse(WORLD, None).unwrap();
    let key = &world
        .imports
        .iter()
        .find(|i| i.name.ends_with("/items"))
        .unwrap()
        .resources["item"];
    json!({"op":"call","interface":"","function":"run","args":[{"$hostResource":format!("loan-{ticket}"),"type":key,"generation":1,"owned":true,"new":true},{"$bigint":ticket.to_string()}]})
}

#[test]
fn ffi_driver_cancelled_queued_forwarding_releases_its_import_borrow_pin() {
    unsafe {
        let mut state = State::default();
        borrowed_imports(&mut state);
        let owner = submit(state.host, owned_call(7)).unwrap();
        poll(&mut state, 10);
        let forwarded = submit(
            state.host,
            json!({"op":"call","interface":"","function":"inspect","args":[state.loans[&7]]}),
        )
        .unwrap();
        let mut out = Buffer::default();
        let status =
            perry_component_driver_cancel(state.host, forwarded.parse().unwrap(), &mut out);
        reply(status, out).unwrap();
        resolve(&mut state, 7, true);
        let done = poll(&mut state, 20);
        assert_eq!(done["completed"].as_array().unwrap().len(), 1, "{done}");
        assert_eq!(done["completed"][0]["id"], owner);
        assert_eq!(done["completed"][0]["ok"], true);
        assert_eq!(done["retired"], false);
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            1
        );
        finish(&mut state);
    }
}

#[test]
fn ffi_driver_retiring_pending_import_borrows_cancels_forwarding_and_drops_owners_once() {
    unsafe {
        let mut state = State::default();
        borrowed_imports(&mut state);
        let owner = submit(state.host, owned_call(7)).unwrap();
        let sibling = submit(state.host, owned_call(8)).unwrap();
        poll(&mut state, 10);
        let requests = state.requests.clone();
        let forwarded = submit(
            state.host,
            json!({"op":"call","interface":"","function":"inspect","args":[state.loans[&7]]}),
        )
        .unwrap();
        let mut out = Buffer::default();
        let status = perry_component_driver_cancel(state.host, owner.parse().unwrap(), &mut out);
        reply(status, out).unwrap();
        let done = poll(&mut state, 20);
        assert_eq!(done["retired"], true);
        assert_eq!(done["retained"], json!([]));
        let results = done["completed"].as_array().unwrap();
        assert_eq!(results.len(), 2, "{done}");
        for id in [sibling, forwarded] {
            assert_eq!(results.iter().find(|r| r["id"] == id).unwrap()["ok"], false);
        }
        let drops: Vec<_> = state
            .resources
            .iter()
            .filter(|(e, _, _)| e["kind"] == "drop")
            .collect();
        assert_eq!(drops.len(), 2);
        assert!(drops
            .iter()
            .all(|(_, origin, frame)| *origin == 0 && *frame == 1020));
        for request in requests.into_values() {
            let mut out = Buffer::default();
            let status = perry_component_host_async_import_resolve(
                state.host,
                request,
                true,
                b"null".as_ptr(),
                4,
                &mut out,
            );
            assert!(reply(status, out).is_err());
        }
        finish(&mut state);
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            2
        );
    }
}

pub(super) fn network_fixture() -> (String, String) {
    let world = r#"package wasi:sockets@0.2.12;
            interface network { resource network; }
            interface instance-network { use network.{network}; instance-network: func() -> own<network>; }
            world plugin {
                import network; import instance-network; use network.{network};
                import loan: async func(value: borrow<network>, ticket: u64);
                import ping: func(value: borrow<network>) -> u32;
                export run: async func(owner: own<network>, value: u64) -> u64;
                export inspect: func(value: borrow<network>) -> u32;
                export get-network: func() -> own<network>;
            }"#;
    let extra = r#"
            (import "wasi:sockets/instance-network@0.2.12" (instance $factory
                (export "instance-network" (func (result (own $item))))))
            (alias export $factory "instance-network" (func $factory))
            (core func $factory (canon lower (func $factory)))
            (core module $factory-code
                (import "host" "get" (func $get (result i32)))
                (func (export "get") (result i32) (call $get)))
            (core instance $factory-code (instantiate $factory-code
                (with "host" (instance (export "get" (func $factory))))))
            (func (export "get-network") (result (own $item))
                (canon lift (core func $factory-code "get")))
        "#;
    let text = borrowed_text()
        .replace("test:borrowed/items", "wasi:sockets/network@0.2.12")
        .replace("\"item\"", "\"network\"")
        .replace(
            "        (import \"ping\"",
            &format!("{extra}        (import \"ping\""),
        );

    (world.to_string(), text)
}

#[test]
fn ffi_driver_wasi_resource_import_borrows_use_native_aliases_until_forwarding_finishes() {
    unsafe {
        let (world, text) = network_fixture();
        let mut state = State::default();
        new_options(&mut state, &world, &text, br#"{"wasi":{}}"#);
        for ticket in 7..10 {
            let resource = completed(
                &mut state,
                json!({"op":"call","interface":"","function":"get-network","args":[]}),
            );
            let owner = submit(state.host, json!({"op":"call","interface":"","function":"run","args":[resource,{"$bigint":ticket.to_string()}]})).unwrap();
            assert_eq!(poll(&mut state, 10)["completed"], json!([]));
            let loan = state.loans[&ticket].clone();
            assert_eq!(loan["owned"], false);
            assert_eq!(loan["$resource"], loan["$borrow"]);
            let forwarded = submit(
                state.host,
                json!({"op":"call","interface":"","function":"inspect","args":[loan]}),
            )
            .unwrap();
            resolve(&mut state, ticket, true);
            let done = poll(&mut state, 20);
            let results = done["completed"].as_array().unwrap();
            assert_eq!(results.len(), 2, "{done}");
            for id in [owner, forwarded.clone()] {
                assert_eq!(
                    results.iter().find(|r| r["id"] == id).unwrap()["ok"],
                    true,
                    "{done}"
                );
            }
            assert_eq!(
                results.iter().find(|r| r["id"] == forwarded).unwrap()["value"],
                42
            );
            assert_eq!(done["retired"], false);
            assert!(submit(
                state.host,
                json!({"op":"call","interface":"","function":"inspect","args":[loan]})
            )
            .is_err());
        }
        finish(&mut state);
    }
}
