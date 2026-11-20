//! Ownership receipts report canonical lower commits, not RPC completion or
//! callback settlement. Failed preflight and declined receipts retain owners.
use super::*;

fn lowers(state: &State) -> Vec<&(Value, u64, u64)> {
    state
        .resources
        .iter()
        .filter(|(event, _, _)| event["kind"] == "lower")
        .collect()
}

#[test]
fn ffi_driver_guest_transfer_receipts_follow_validation_and_can_reject_without_consuming() {
    unsafe {
        const NS: &str = "perry:resources/counters@1.0.0";
        let mut state = State::default();
        new(
            &mut state,
            crate::resource_tests::WORLD,
            crate::resource_tests::COMPONENT,
        );
        let resource = completed(
            &mut state,
            json!({"op":"call","interface":NS,"function":"[constructor]counter","args":[17]}),
        );
        let token = resource["$resource"].clone();
        let invalid = submit(state.host, json!({"op":"call","interface":NS,"function":"consume-two","args":[resource,{"$resource":"unknown-owner","owned":true}]})).unwrap();
        let done = poll(&mut state, 10);
        assert_eq!(done["completed"][0]["id"], invalid);
        assert_eq!(done["completed"][0]["ok"], false);
        assert_eq!(done["retired"], false);
        assert!(
            lowers(&state).is_empty(),
            "invalid later argument cannot publish a commit"
        );
        state.reject_lower = true;
        let rejected = submit(
            state.host,
            json!({"op":"call","interface":NS,"function":"consume","args":[resource]}),
        )
        .unwrap();
        let done = poll(&mut state, 20);
        assert_eq!(done["completed"][0]["id"], rejected);
        assert_eq!(done["completed"][0]["ok"], false);
        assert_eq!(done["retired"], false);
        assert_eq!(done["failedOrigins"], json!([rejected]));
        assert!(state.guest_drops.is_empty());
        state.reject_lower = false;
        assert_eq!(
            completed(
                &mut state,
                json!({"op":"call","interface":NS,"function":"[method]counter.read","args":[resource]})
            ),
            17
        );
        let accepted = submit(
            state.host,
            json!({"op":"call","interface":NS,"function":"consume","args":[resource]}),
        )
        .unwrap();
        let done = poll(&mut state, 30);
        assert_eq!(done["completed"][0]["id"], accepted);
        assert_eq!(done["completed"][0]["ok"], true);
        let events = lowers(&state);
        assert_eq!(events.len(), 2, "one receipt for each attempted commit");
        for (event, _, _) in &events {
            assert_eq!(event["guestTransferred"], json!([token]));
            assert_eq!(event["created"], json!([]));
            assert_eq!(event["transferred"], json!([]));
            assert!(event.get("asyncTransferred").is_none());
        }
        assert_eq!(events[0].1, rejected.parse::<u64>().unwrap());
        assert_eq!(events[1].1, accepted.parse::<u64>().unwrap());
        assert_eq!(events[1].2, 30);
        assert_eq!(state.guest_drops, [(17, accepted.parse().unwrap(), 30)]);
        finish(&mut state);
    }
}

#[test]
fn ffi_driver_future_and_stream_transfer_receipts_preserve_owners_on_rejection() {
    unsafe {
        for (kind, function, key) in [
            ("future", "void", "$future"),
            ("stream", "units", "$stream"),
        ] {
            let mut state = State::default();
            new(
                &mut state,
                SCALAR_WORLD,
                include_str!("../../../async_values/factory-echo.component.wat"),
            );
            let mut out = Buffer::default();
            let status = perry_component_driver_types(state.host, &mut out);
            let types = reply(status, out).unwrap();
            let index = types
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["schema"]["kind"] == kind && t["schema"]["value"].is_null())
                .unwrap()["index"]
                .clone();
            let pair = completed(&mut state, json!({"op":"create","index":index}));
            state.reject_lower = true;
            let rejected = submit(
                state.host,
                json!({"op":"call","interface":"","function":function,"args":[pair["value"]]}),
            )
            .unwrap();
            let done = poll(&mut state, 10);
            assert_eq!(done["completed"][0]["id"], rejected);
            assert_eq!(done["completed"][0]["ok"], false);
            assert_eq!(done["retired"], false);
            state.reject_lower = false;
            let accepted = submit(
                state.host,
                json!({"op":"call","interface":"","function":function,"args":[pair["value"]]}),
            )
            .unwrap();
            let done = poll(&mut state, 20);
            assert_eq!(done["completed"][0]["id"], accepted);
            assert_eq!(done["completed"][0]["ok"], true, "{done}");
            let returned = done["completed"][0]["value"].clone();
            assert_ne!(returned[key], pair["value"][key]);
            let events = lowers(&state);
            assert_eq!(events.len(), 2);
            for (event, _, _) in &events {
                assert_eq!(event["asyncTransferred"], json!([pair["value"][key]]));
                assert!(event.get("guestTransferred").is_none());
            }
            assert_eq!(events[1].1, accepted.parse::<u64>().unwrap());
            assert_eq!(events[1].2, 20);
            let write = submit(
                state.host,
                json!({"op":"write","writer":pair["writer"],"values":[null]}),
            )
            .unwrap();
            let read = submit(
                state.host,
                json!({"op":"read","value":returned,"capacity":1}),
            )
            .unwrap();
            let done = poll(&mut state, 30);
            for id in [write, read] {
                assert_eq!(
                    done["completed"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|r| r["id"] == id)
                        .unwrap()["ok"],
                    true,
                    "{done}"
                );
            }
            if kind == "stream" {
                completed(
                    &mut state,
                    json!({"op":"close-writer","writer":pair["writer"]}),
                );
                completed(&mut state, json!({"op":"close","value":returned}));
            }
            finish(&mut state);
        }
    }
}

#[test]
fn ffi_driver_native_only_future_clients_do_not_require_a_lifecycle_callback() {
    unsafe {
        let world = perry_wit::parse(SCALAR_WORLD, None).unwrap();
        let contract = serde_json::to_vec(&world).unwrap();
        let wasm = wat::parse_str(include_str!(
            "../../../async_values/factory-echo.component.wat"
        ))
        .unwrap();
        let mut error = Buffer::default();
        let handle = perry_component_driver_new(
            wasm.as_ptr(),
            wasm.len(),
            contract.as_ptr(),
            contract.len(),
            std::ptr::null(),
            0,
            None,
            None,
            None,
            99,
            &mut error,
        );
        let mut state = State {
            host: handle,
            ..State::default()
        };
        assert!(
            !state.host.is_null(),
            "{}",
            String::from_utf8_lossy(bytes(error.data, error.len).unwrap())
        );
        perry_component_host_buffer_drop(error);
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
        let pair = completed(&mut state, json!({"op":"create","index":index}));
        let echoed = completed(
            &mut state,
            json!({"op":"call","interface":"","function":"void","args":[pair["value"]]}),
        );
        assert_ne!(echoed["$future"], pair["value"]["$future"]);
        finish(&mut state);
    }
}

#[test]
fn ffi_driver_import_result_ownership_commits_after_accepted_borrowers_return() {
    unsafe {
        let (world, mut text) = interop_tests::network_fixture();
        let world = world.replace("ticket: u64);", "ticket: u64) -> own<network>;");
        fn replace(text: &mut String, from: &str, to: &str) {
            assert_eq!(text.matches(from).count(), 1, "fixture replacement {from}");
            *text = text.replacen(from, to, 1);
        }
        replace(
            &mut text,
            "(param \"ticket\" u64)))",
            "(param \"ticket\" u64) (result (own $item))))",
        );
        replace(
            &mut text,
            "(core func $wait (canon lower (func $loan) async))",
            "",
        );
        replace(&mut text, "(core instance $heap (instantiate $heap))", "(core instance $heap (instantiate $heap)) (core func $wait (canon lower (func $loan) async (memory (core memory $heap \"memory\"))))");
        replace(
            &mut text,
            "(func $wait (param i32 i64) (result i32))",
            "(func $wait (param i32 i64 i32) (result i32))",
        );
        replace(&mut text, "(call $wait (i32.load offset=16 (local.get $frame)) (local.get $value))", "(call $wait (i32.load offset=16 (local.get $frame)) (local.get $value) (i32.add (local.get $frame) (i32.const 20)))");
        replace(&mut text, "(call $drop-item (i32.load offset=16 (local.get $frame)))", "(call $drop-item (i32.load offset=20 (local.get $frame))) (call $drop-item (i32.load offset=16 (local.get $frame)))");
        let mut state = State {
            forward_during_loan: true,
            ..State::default()
        };
        new_options(&mut state, &world, &text, br#"{"wasi":{}}"#);
        let owner = completed(
            &mut state,
            json!({"op":"call","interface":"","function":"get-network","args":[]}),
        );
        let returned = completed(
            &mut state,
            json!({"op":"call","interface":"","function":"get-network","args":[]}),
        );
        state.watched_transfer = Some(returned["$resource"].clone());
        state.loan_result = Some(returned.clone());
        state.resources.clear();
        let running = submit(
            state.host,
            json!({"op":"call","interface":"","function":"run","args":[owner,{"$bigint":"7"}]}),
        )
        .unwrap();
        let mut results = Vec::new();
        for marker in 10..138 {
            let done = poll(&mut state, marker);
            assert_eq!(done["retired"], false, "{done}");
            results.extend(done["completed"].as_array().unwrap().iter().cloned());
            if results.len() == 2 {
                break;
            }
        }
        assert_eq!(results.len(), 2);
        assert!(!state.early_commit);
        assert_eq!(state.pings, 1);
        for id in [&running, state.nested.as_ref().unwrap()] {
            assert_eq!(
                results.iter().find(|r| r["id"] == *id).unwrap()["ok"],
                true,
                "{results:?}"
            );
        }
        let commits = lowers(&state);
        assert_eq!(commits.len(), 2);
        assert_eq!(
            commits[0].0["guestTransferred"],
            json!([owner["$resource"]])
        );
        assert_eq!(
            commits[1].0["guestTransferred"],
            json!([returned["$resource"]])
        );
        assert_eq!(commits[1].1, running.parse::<u64>().unwrap());
        finish(&mut state);
    }
}
