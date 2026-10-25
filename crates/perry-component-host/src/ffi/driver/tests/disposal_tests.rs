use super::*;
const NS: &str = "perry:resources/counters@1.0.0";
unsafe fn guest(state: &mut State, value: u32) -> Value {
    completed(
        state,
        json!({"op":"call","interface":NS,"function":"[constructor]counter","args":[value]}),
    )
}
unsafe fn collect(state: &mut State, request: Value) {
    completed(state, json!({"op":"resource-collect","request":request}));
}
#[test]
fn ffi_driver_resource_disposal_preserves_nominal_identity_and_ignores_transferred_collectors() {
    unsafe {
        let mut state = State::default();
        new(
            &mut state,
            crate::resource_tests::WORLD,
            crate::resource_tests::COMPONENT,
        );
        let resource = guest(&mut state, 17);
        let token = resource["$resource"].as_str().unwrap();
        let drop = submit(state.host, json!({"op":"resource-drop","value":resource})).unwrap();
        let done = poll(&mut state, 30);
        assert_eq!(
            done["completed"],
            json!([{"id":drop,"ok":true,"returned":false}])
        );
        assert_eq!(state.guest_drops, vec![(17, drop.parse().unwrap(), 30)]);
        collect(&mut state, json!({"kind":"guest","token":token}));
        assert_eq!(
            state.guest_drops.len(),
            1,
            "late collector must not repeat destruction"
        );
        let moved = guest(&mut state, 18);
        completed(
            &mut state,
            json!({"op":"call","interface":NS,"function":"consume","args":[moved]}),
        );
        assert_eq!(state.guest_drops.len(), 2);
        collect(
            &mut state,
            json!({"kind":"guest","token":moved["$resource"]}),
        );
        assert_eq!(
            state.guest_drops.len(),
            2,
            "transferred identity must not be reclaimed"
        );
        let bad = submit(state.host, json!({"op":"resource-drop","value":resource})).unwrap();
        let done = poll(&mut state, 40);
        assert_eq!(done["completed"][0]["id"], bad);
        assert_eq!(done["completed"][0]["ok"], false);
        assert_eq!(
            done["retired"], false,
            "invalid explicit drop is a preflight rejection"
        );
        let live = guest(&mut state, 19);
        collect(
            &mut state,
            json!({"kind":"guest","token":live["$resource"]}),
        );
        assert_eq!(state.guest_drops.len(), 3);
        // Plain resource disposal adds no invented future/stream factory types.
        let mut out = Buffer::default();
        let status = perry_component_driver_types(state.host, &mut out);
        assert_eq!(reply(status, out).unwrap(), json!([]));
        finish(&mut state);
    }
}
#[test]
fn ffi_driver_foreign_disposal_preflight_and_discarded_cleanup_observer_keep_owners_distinct() {
    unsafe {
        let mut left = State::default();
        let mut right = State::default();
        new(
            &mut left,
            crate::resource_tests::WORLD,
            crate::resource_tests::COMPONENT,
        );
        new(
            &mut right,
            crate::resource_tests::WORLD,
            crate::resource_tests::COMPONENT,
        );
        let resource = guest(&mut left, 22);
        let foreign = guest(&mut right, 23);
        let id = submit(left.host, json!({"op":"resource-drop","value":foreign})).unwrap();
        let failed = poll(&mut left, 10);
        assert_eq!(failed["completed"][0]["id"], id);
        assert_eq!(failed["completed"][0]["ok"], false);
        assert_eq!(failed["retired"], false);
        assert!(right.guest_drops.is_empty());
        let id = submit(left.host, json!({"op":"resource-drop","value":resource})).unwrap();
        let mut out = Buffer::default();
        let status = perry_component_driver_cancel(left.host, id.parse().unwrap(), &mut out);
        reply(status, out).unwrap();
        let done = poll(&mut left, 20);
        assert_eq!(done["completed"], json!([]));
        assert_eq!(done["retired"], false);
        assert_eq!(
            left.guest_drops,
            vec![(22, id.parse().unwrap(), 20)],
            "accepted disposal executes after observer drop"
        );
        collect(
            &mut right,
            json!({"kind":"guest","token":foreign["$resource"]}),
        );
        assert_eq!(right.guest_drops.len(), 1);
        finish(&mut left);
        finish(&mut right);
    }
}
#[test]
fn ffi_driver_resource_destructor_failure_retires_store_and_preserves_original_origin() {
    unsafe {
        let mut state = State::default();
        new(
            &mut state,
            crate::resource_tests::WORLD,
            crate::resource_tests::COMPONENT,
        );
        let resource = guest(&mut state, 29);
        state.reject_drop = true;
        let id = submit(state.host, json!({"op":"resource-drop","value":resource})).unwrap();
        let failed = poll(&mut state, 60);
        assert_eq!(failed["retired"], true);
        assert_eq!(failed["failedOrigins"], json!([id]));
        assert!(failed["completed"][0]["error"]
            .as_str()
            .unwrap()
            .contains("guest destructor rejected"));
        assert_eq!(state.guest_drops.len(), 1);
        finish(&mut state);
        assert_eq!(state.guest_drops.len(), 1);
    }
}
#[test]
fn ffi_driver_host_resource_collectors_validate_generation_and_guest_ownership() {
    unsafe {
        let mut state = State::default();
        new(
            &mut state,
            crate::host_resource_tests::WORLD,
            crate::host_resource_tests::COMPONENT,
        );
        let world = perry_wit::parse(crate::host_resource_tests::WORLD, None).unwrap();
        let key = &world.imports[0].resources["token"];
        let fresh = |token: &str| json!({"$hostResource":token,"type":key,"generation":1,"owned":true,"new":true});
        let returned = completed(
            &mut state,
            json!({"op":"call","interface":"","function":"echo","args":[fresh("renewed")]}),
        );
        assert_eq!(returned["generation"], 2);
        let reference = |token: &str, generation: u32| json!({"token":token,"type":key,"generation":generation});
        collect(
            &mut state,
            json!({"kind":"host","resource":reference("renewed",1)}),
        );
        assert!(!state.resources.iter().any(|(e, _, _)| e["kind"] == "drop"));
        let id=submit(state.host,json!({"op":"resource-collect","request":{"kind":"host","resource":reference("renewed",2)}})).unwrap();
        assert_eq!(poll(&mut state, 31)["completed"][0]["ok"], true);
        let drops: Vec<_> = state
            .resources
            .iter()
            .filter(|(e, _, _)| e["kind"] == "drop")
            .collect();
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].1, id.parse::<u64>().unwrap());
        assert_eq!(drops[0].2, 31);
        collect(
            &mut state,
            json!({"kind":"host","resource":reference("renewed",2)}),
        );
        completed(
            &mut state,
            json!({"op":"call","interface":"","function":"keep","args":[fresh("held")]}),
        );
        collect(
            &mut state,
            json!({"kind":"host","resource":reference("held",1)}),
        );
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            1,
            "collector cannot take guest-owned payloads"
        );
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

#[test]
fn ffi_driver_disposes_a_host_resource_while_another_guest_call_is_suspended() {
    unsafe {
        let (source, text) = fixture_text(false);
        let source=source.replace("world plugin {","interface tokens { resource token; } world plugin { import tokens; use tokens.{token}; export echo: func(value: own<token>) -> own<token>;");
        let text=text.replacen("(component",r#"(component
            (import "test:driver/tokens" (instance $tokens (export "token" (type $token (sub resource)))))
            (alias export $tokens "token" (type $token))
            (core module $echo (func (export "echo") (param i32) (result i32) (local.get 0)))
            (core instance $echo (instantiate $echo))
            (func (export "echo") (param "value" (own $token)) (result (own $token)) (canon lift (core func $echo "echo")))
        "#,1);
        let mut state = State::default();
        new(&mut state, &source, &text);
        let world = perry_wit::parse(&source, None).unwrap();
        let key = &world
            .imports
            .iter()
            .find(|i| i.name.ends_with("/tokens"))
            .unwrap()
            .resources["token"];
        let value =
            json!({"$hostResource":"concurrent","type":key,"generation":1,"owned":true,"new":true});
        let resource = completed(
            &mut state,
            json!({"op":"call","interface":"","function":"echo","args":[value]}),
        );
        let running = submit(state.host, call(7)).unwrap();
        assert_eq!(poll(&mut state, 10)["completed"], json!([]));
        assert!(state.requests.contains_key(&7));
        let disposal = submit(state.host, json!({"op":"resource-drop","value":resource})).unwrap();
        let done = poll(&mut state, 20);
        assert_eq!(
            done["completed"],
            json!([{"id":disposal,"ok":true,"returned":false}])
        );
        assert_eq!(done["active"], true);
        let drops: Vec<_> = state
            .resources
            .iter()
            .filter(|(e, _, _)| e["kind"] == "drop")
            .collect();
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].1, disposal.parse::<u64>().unwrap());
        assert_eq!(drops[0].2, 20);
        resolve(&mut state, 7, true);
        let done = poll(&mut state, 30);
        assert_eq!(done["completed"][0]["id"], running);
        assert_eq!(done["retired"], false);
        finish(&mut state);
    }
}

#[test]
fn ffi_driver_typed_resource_disposal_separates_colliding_guest_and_host_tokens() {
    unsafe {
        let source=crate::resource_tests::WORLD.replace("world plugin {","interface tokens { resource token; } world plugin { import tokens; use tokens.{token}; export echo-host: func(value: own<token>) -> own<token>;");
        let text=crate::resource_tests::COMPONENT.replacen("(component",r#"(component
            (import "perry:resources/tokens@1.0.0" (instance $tokens (export "token" (type $token (sub resource)))))
            (alias export $tokens "token" (type $token))
            (core module $echo (func (export "echo") (param i32) (result i32) (local.get 0)))
            (core instance $echo (instantiate $echo))
            (func (export "echo-host") (param "value" (own $token)) (result (own $token)) (canon lift (core func $echo "echo")))
        "#,1);
        let mut state = State::default();
        new(&mut state, &source, &text);
        let resource = guest(&mut state, 71);
        let world = perry_wit::parse(&source, None).unwrap();
        let key = &world
            .imports
            .iter()
            .find(|i| i.name.contains("/tokens@"))
            .unwrap()
            .resources["token"];
        let collision = resource["$resource"].as_str().unwrap();
        let host =
            json!({"$hostResource":collision,"type":key,"generation":1,"owned":true,"new":true});
        let host = completed(
            &mut state,
            json!({"op":"call","interface":"","function":"echo-host","args":[host]}),
        );
        assert_eq!(host["$hostResource"], resource["$resource"]);
        assert!(submit(
            state.host,
            json!({"op":"resource-drop","value":{"$resource":collision,"owned":false}})
        )
        .is_err());
        assert!(submit(state.host,json!({"op":"resource-drop","value":{"$resource":collision,"$hostResource":collision,"owned":true}})).is_err());
        completed(&mut state, json!({"op":"resource-drop","value":resource}));
        assert_eq!(state.guest_drops.len(), 1);
        assert!(
            !state.resources.iter().any(|(e, _, _)| e["kind"] == "drop"),
            "guest disposal must not select a same-spelled host token"
        );
        let renewed = completed(
            &mut state,
            json!({"op":"call","interface":"","function":"echo-host","args":[host]}),
        );
        let stale = submit(state.host, json!({"op":"resource-drop","value":host})).unwrap();
        let rejected = poll(&mut state, 50);
        assert_eq!(rejected["completed"][0]["id"], stale);
        assert_eq!(rejected["completed"][0]["ok"], false);
        assert_eq!(rejected["retired"], false);
        completed(&mut state, json!({"op":"resource-drop","value":renewed}));
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            1
        );
        assert_eq!(state.guest_drops.len(), 1);
        finish(&mut state);
    }
}

unsafe fn void_pair(state: &mut State) -> Value {
    let mut out = Buffer::default();
    let status = perry_component_driver_types(state.host, &mut out);
    let types = reply(status, out).unwrap();
    let index = types
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["schema"]["kind"] == "future" && t["schema"]["value"].is_null())
        .unwrap()["index"]
        .clone();
    completed(state, json!({"op":"create","index":index}))
}
#[test]
fn ffi_driver_async_collectors_ignore_transferred_and_consumed_readable_tokens() {
    unsafe {
        let mut state = State::default();
        new(
            &mut state,
            SCALAR_WORLD,
            include_str!("../../../async_values/factory-echo.component.wat"),
        );
        let pair = void_pair(&mut state);
        let moved = completed(
            &mut state,
            json!({"op":"call","interface":"","function":"void","args":[pair["value"]]}),
        );
        assert_ne!(moved, pair["value"]);
        completed(&mut state, json!({"op":"collect","value":pair["value"]}));
        let write = submit(
            state.host,
            json!({"op":"write","writer":pair["writer"],"values":[null]}),
        )
        .unwrap();
        let read = submit(state.host, json!({"op":"read","value":moved})).unwrap();
        let done = poll(&mut state, 10);
        let events = done["completed"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events.iter().find(|v| v["id"] == write).unwrap()["value"]["written"],
            1
        );
        assert_eq!(
            events.iter().find(|v| v["id"] == read).unwrap()["returned"],
            false
        );
        for _ in 0..2 {
            completed(&mut state, json!({"op":"collect","value":moved}));
        }
        let close = submit(state.host, json!({"op":"close","value":moved})).unwrap();
        let rejected = poll(&mut state, 20);
        assert_eq!(rejected["completed"][0]["id"], close);
        assert_eq!(rejected["completed"][0]["ok"], false);
        assert_eq!(rejected["retired"], false);
        finish(&mut state);
    }
}
#[test]
fn ffi_driver_async_collection_cancels_pending_reads_after_its_observer_is_discarded() {
    unsafe {
        let mut state = State::default();
        new(
            &mut state,
            SCALAR_WORLD,
            include_str!("../../../async_values/factory-echo.component.wat"),
        );
        let pair = void_pair(&mut state);
        let read = submit(state.host, json!({"op":"read","value":pair["value"]})).unwrap();
        assert_eq!(poll(&mut state, 10)["completed"], json!([]));
        let collect = submit(state.host, json!({"op":"collect","value":pair["value"]})).unwrap();
        let mut out = Buffer::default();
        let status = perry_component_driver_cancel(state.host, collect.parse().unwrap(), &mut out);
        reply(status, out).unwrap();
        let done = poll(&mut state, 20);
        assert_eq!(done["completed"].as_array().unwrap().len(), 1);
        assert_eq!(done["completed"][0]["id"], read);
        assert_eq!(done["completed"][0]["ok"], false);
        assert_eq!(done["retired"], false);
        let written = completed(
            &mut state,
            json!({"op":"write","writer":pair["writer"],"values":[null]}),
        );
        assert_eq!(written["written"], 0);
        assert_eq!(written["remaining"], json!([null]));
        assert_eq!(written["closed"], true);
        finish(&mut state);
    }
}

unsafe fn borrowed_fixture(state: &mut State, early: bool) -> Value {
    let (source, text) = fixture_text(early);
    let source=source.replace("world plugin {","interface tokens { resource token; } world plugin { import tokens; use tokens.{token}; export echo: func(value: own<token>) -> own<token>;")
        .replace("export run: async func(value: u64)","export run: async func(loan: borrow<token>, value: u64)");
    let mut text=text.replacen("(component",r#"(component
        (import "test:driver/tokens" (instance $tokens (export "token" (type $token (sub resource)))))
        (alias export $tokens "token" (type $token))
        (core func $drop-loan (canon resource.drop $token))
        (core module $echo (func (export "echo") (param i32) (result i32) (local.get 0)))
        (core instance $echo (instantiate $echo))
        (func (export "echo") (param "value" (own $token)) (result (own $token)) (canon lift (core func $echo "echo")))
    "#,1)
        .replace("(func (export \"run\") async (param \"value\" u64)","(func (export \"run\") async (param \"loan\" (borrow $token)) (param \"value\" u64)")
        .replace("(param $value i64) (result i32)","(param $loan i32) (param $value i64) (result i32)")
        .replace("(core module $code","(core module $code (import \"host\" \"drop-loan\" (func $drop-loan (param i32)))")
        .replace("(export \"get\" (func $get))","(export \"drop-loan\" (func $drop-loan)) (export \"get\" (func $get))")
        .replace("(call $alloc (i32.const 16))","(call $alloc (i32.const 24))")
        .replace("(i64.store (local.get $frame) (local.get $value))","(i64.store (local.get $frame) (local.get $value)) (i32.store offset=16 (local.get $frame) (local.get $loan))");
    if early {
        text = text.replace(
            "(call $return (local.get $value))",
            "(call $drop-loan (local.get $loan)) (call $return (local.get $value))",
        );
    } else {
        text=text.replace("(call $return (i64.load (local.get $frame)))","(call $drop-loan (i32.load offset=16 (local.get $frame))) (call $return (i64.load (local.get $frame)))");
    }
    new(state, &source, &text);
    let world = perry_wit::parse(&source, None).unwrap();
    let key = &world
        .imports
        .iter()
        .find(|i| i.name.ends_with("/tokens"))
        .unwrap()
        .resources["token"];
    completed(
        state,
        json!({"op":"call","interface":"","function":"echo","args":[{"$hostResource":"borrowed","type":key,"generation":1,"owned":true,"new":true}]}),
    )
}
fn borrow_call(resource: &Value, value: u64) -> Value {
    json!({"op":"call","interface":"","function":"run","args":[resource,{"$bigint":value.to_string()}]})
}
#[test]
fn ffi_driver_collection_waits_for_all_active_borrows_and_explicit_disposal_rejects_safely() {
    unsafe {
        let mut state = State::default();
        let resource = borrowed_fixture(&mut state, false);
        let first = submit(state.host, borrow_call(&resource, 7)).unwrap();
        let second = submit(state.host, borrow_call(&resource, 8)).unwrap();
        assert_eq!(poll(&mut state, 10)["completed"], json!([]));
        let rejected = submit(state.host, json!({"op":"resource-drop","value":resource})).unwrap();
        let done = poll(&mut state, 20);
        assert_eq!(done["completed"][0]["id"], rejected);
        assert_eq!(done["completed"][0]["ok"], false);
        assert_eq!(done["retired"], false);
        assert!(done["completed"][0]["error"]
            .as_str()
            .unwrap()
            .contains("borrowed by an active"));
        let moved = submit(
            state.host,
            json!({"op":"call","interface":"","function":"echo","args":[resource]}),
        )
        .unwrap();
        let done = poll(&mut state, 25);
        assert_eq!(done["completed"][0]["id"], moved);
        assert_eq!(done["completed"][0]["ok"], false);
        assert_eq!(done["retired"], false);
        let collector=submit(state.host,json!({"op":"resource-collect","request":{"kind":"host","resource":{"token":resource["$hostResource"],"type":resource["type"],"generation":resource["generation"]}}})).unwrap();
        assert_eq!(poll(&mut state, 30)["completed"], json!([]));
        resolve(&mut state, 7, true);
        let done = poll(&mut state, 40);
        assert_eq!(done["completed"].as_array().unwrap().len(), 1);
        assert_eq!(done["completed"][0]["id"], first);
        assert!(
            !state.resources.iter().any(|(e, _, _)| e["kind"] == "drop"),
            "the second call still owns a borrow"
        );
        resolve(&mut state, 8, true);
        let done = poll(&mut state, 50);
        let events = done["completed"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        for id in [&second, &collector] {
            assert_eq!(events.iter().find(|e| e["id"] == *id).unwrap()["ok"], true);
        }
        let drops: Vec<_> = state
            .resources
            .iter()
            .filter(|(e, _, _)| e["kind"] == "drop")
            .collect();
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].1, collector.parse::<u64>().unwrap());
        assert_eq!(drops[0].2, 50);
        finish(&mut state);
    }
}
#[test]
fn ffi_driver_borrow_ownership_returns_with_the_result_before_background_task_exit() {
    unsafe {
        let mut state = State::default();
        let resource = borrowed_fixture(&mut state, true);
        let running = submit(state.host, borrow_call(&resource, 7)).unwrap();
        let done = poll(&mut state, 10);
        assert_eq!(done["completed"][0]["id"], running);
        assert_eq!(done["active"], true);
        let id = submit(state.host, json!({"op":"resource-drop","value":resource})).unwrap();
        let done = poll(&mut state, 20);
        assert_eq!(done["completed"][0]["id"], id);
        assert_eq!(done["completed"][0]["ok"], true);
        assert_eq!(done["retired"], false);
        assert_eq!(done["active"], true);
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            1
        );
        resolve(&mut state, 7, true);
        assert_eq!(poll(&mut state, 30)["retired"], false);
        finish(&mut state);
    }
}

#[test]
fn ffi_driver_retirement_releases_waiting_collectors_and_borrow_guards_once() {
    unsafe {
        let mut state = State::default();
        let resource = borrowed_fixture(&mut state, false);
        let running = submit(state.host, borrow_call(&resource, 7)).unwrap();
        poll(&mut state, 10);
        let request = state.requests[&7];
        let collector=submit(state.host,json!({"op":"resource-collect","request":{"kind":"host","resource":{"token":resource["$hostResource"],"type":resource["type"],"generation":resource["generation"]}}})).unwrap();
        assert_eq!(poll(&mut state, 20)["completed"], json!([]));
        let mut out = Buffer::default();
        let status = perry_component_driver_cancel(state.host, running.parse().unwrap(), &mut out);
        reply(status, out).unwrap();
        let done = poll(&mut state, 30);
        assert_eq!(done["retired"], true);
        assert_eq!(done["retained"], json!([]));
        assert_eq!(done["completed"][0]["id"], collector);
        assert_eq!(done["completed"][0]["ok"], false);
        let drops: Vec<_> = state
            .resources
            .iter()
            .filter(|(e, _, _)| e["kind"] == "drop")
            .collect();
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].1, 0);
        assert_eq!(drops[0].2, 1030);
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
        finish(&mut state);
        assert_eq!(
            state
                .resources
                .iter()
                .filter(|(e, _, _)| e["kind"] == "drop")
                .count(),
            1
        );
    }
}
