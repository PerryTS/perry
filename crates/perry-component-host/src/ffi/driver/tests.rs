use super::*;
use perry_abi::component_host::ASYNC_PENDING;

#[derive(Default)]
struct State {
    host: *mut c_void,
    requests: BTreeMap<u64, u64>,
    seen: Vec<(u64, u64, u64)>, // value, origin, fresh poll token marker
    nested: Option<String>,
    reentry_rejected: bool,
    resources: Vec<(Value, u64, u64)>,
    reject_drop: bool,
}
struct Frame {
    state: *mut State,
    marker: u64,
}
unsafe fn reply(status: i32, out: Buffer) -> Result<Value> {
    let value = bytes(out.data, out.len)?.to_vec();
    perry_component_host_buffer_drop(out);
    ensure!(status == OK, "{}", String::from_utf8_lossy(&value));
    Ok(if value.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&value)?
    })
}
unsafe fn submit(host: *mut c_void, value: Value) -> Result<String> {
    let value = serde_json::to_vec(&value)?;
    let mut out = Buffer::default();
    let status = perry_component_driver_submit(host, value.as_ptr(), value.len(), &mut out);
    Ok(reply(status, out)?["id"].as_str().unwrap().into())
}
unsafe extern "C" fn wake(_: u64) {}
unsafe fn poll(state: &mut State, marker: u64) -> Value {
    let primary = Frame { state, marker };
    let cleanup = Frame {
        state,
        marker: marker + 1000,
    };
    let mut out = Buffer::default();
    let status = perry_component_driver_poll(
        state.host,
        &primary as *const Frame as usize as u64,
        &cleanup as *const Frame as usize as u64,
        Some(wake),
        1,
        &mut out,
    );
    reply(status, out).unwrap()
}
unsafe fn resolve(state: &mut State, value: u64, success: bool) {
    let id = state.requests.remove(&value).unwrap();
    let value: &[u8] = if success {
        b"null"
    } else {
        b"expected rejected import"
    };
    let mut out = Buffer::default();
    let status = perry_component_host_async_import_resolve(
        state.host,
        id,
        success,
        value.as_ptr(),
        value.len(),
        &mut out,
    );
    reply(status, out).unwrap();
}
unsafe extern "C" fn callback(
    context: u64,
    origin: u64,
    _: *const u8,
    _: usize,
    function: *const u8,
    function_len: usize,
    args: *const u8,
    args_len: usize,
    out: *mut Buffer,
) -> i32 {
    let frame = &*(context as usize as *const Frame);
    let state = &mut *frame.state;
    if bytes(function, function_len).unwrap() == b"consume" {
        output(out, b"primary synchronous import error".to_vec());
        return ERROR;
    }
    if bytes(function, function_len).unwrap() == b"now" {
        state.seen.push((0, origin, frame.marker));
        output(out, br#"{"$bigint":"0"}"#.to_vec());
        return OK;
    }
    let args: Value = serde_json::from_slice(bytes(args, args_len).unwrap()).unwrap();
    let value: u64 = args["args"][0]["$bigint"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let request: u64 = args["$asyncRequest"].as_str().unwrap().parse().unwrap();
    state.seen.push((value, origin, frame.marker));
    state.requests.insert(value, request);
    if value == 1 {
        // A new command can execute in this very poll. The actor must not hold
        // an aggregate mutable borrow that aliases callback submission.
        state.nested = Some(
            submit(
                state.host,
                json!({"op":"call","interface":"","function":"run","args":[{"$bigint":"3"}]}),
            )
            .unwrap(),
        );
        let mut nested = Buffer::default();
        let status =
            perry_component_driver_poll(state.host, context, context, Some(wake), 1, &mut nested);
        state.reentry_rejected = reply(status, nested)
            .unwrap_err()
            .to_string()
            .contains("re-entered");
    }
    ASYNC_PENDING
}
unsafe fn new(state: &mut State, source: &str, component: &str) {
    let world = perry_wit::parse(source, None).unwrap();
    let wasm = wat::parse_str(component).unwrap();
    let contract = serde_json::to_vec(&world).unwrap();
    let frame = Frame { state, marker: 99 };
    let mut error = Buffer::default();
    state.host = perry_component_driver_new(
        wasm.as_ptr(),
        wasm.len(),
        contract.as_ptr(),
        contract.len(),
        std::ptr::null(),
        0,
        Some(callback),
        Some(perry_component_host_buffer_drop),
        Some(resource_callback),
        &frame as *const Frame as usize as u64,
        &mut error,
    );
    assert!(
        !state.host.is_null(),
        "{}",
        String::from_utf8_lossy(bytes(error.data, error.len).unwrap())
    );
    perry_component_host_buffer_drop(error);
}
unsafe fn fixture(state: &mut State, early: bool) {
    let source = "package test:driver; interface clock { now: func() -> u64; wait-for: async func(value: u64); } world plugin { import clock; export run: async func(value: u64) -> u64; }";
    let world = perry_wit::parse(source, None).unwrap();
    let heap =
        include_str!("../../async_values/heap.wat").replacen("(module", "(core module $heap", 1);
    let mut text = include_str!("../../async_values/task_context.component.wat")
        .replace(";; HEAP", &heap)
        .replace("\"clock\"", &format!("\"{}\"", world.imports[0].name))
        .replace("(instance $clock", "(instance $clock (export \"now\" (func (result u64)))")
        .replace("  (core func $return", "  (alias export $clock \"now\" (func $now))\n  (core func $now (canon lower (func $now)))\n  (core func $return")
        .replace("  (core module $code", "  (core module $code (import \"host\" \"now\" (func $now (result i64)))")
        .replace("      (call $drop-set", "      (drop (call $now))\n      (call $drop-set")
        .replace("    (export \"get\"", "    (export \"now\" (func $now))\n    (export \"get\"");
    if early {
        text = text.replace("      (call $return (i64.load (local.get $frame)))\n", "")
            .replace("      (local.set $status (call $wait (local.get $value)))", "      (call $return (local.get $value))\n      (local.set $status (call $wait (local.get $value)))");
    }
    new(state, source, &text);
}
fn call(value: u64) -> Value {
    json!({"op":"call","interface":"","function":"run","args":[{"$bigint":value.to_string()}]})
}
unsafe fn finish(state: &mut State) {
    let frame = Frame {
        state,
        marker: 2000,
    };
    let mut out = Buffer::default();
    let status =
        perry_component_host_dispose(state.host, &frame as *const Frame as usize as u64, &mut out);
    reply(status, out).unwrap();
    perry_component_host_drop(state.host);
}
#[test]
fn ffi_driver_overlaps_calls_refreshes_origins_and_allows_queue_only_reentry() {
    unsafe {
        let mut state = State::default();
        fixture(&mut state, false);
        let first = submit(state.host, call(1)).unwrap();
        let second = submit(state.host, call(2)).unwrap();
        assert!(
            state.seen.is_empty(),
            "submission must not run guest or callbacks"
        );
        let initial = poll(&mut state, 10);
        assert_eq!(initial["completed"], json!([]));
        assert!(state.reentry_rejected);
        let third = state.nested.clone().unwrap();
        for (value, expected) in [(1, &first), (2, &second), (3, &third)] {
            let observed = state.seen.iter().find(|(v, _, _)| *v == value).unwrap();
            assert_eq!(observed.1.to_string(), *expected);
            assert_eq!(observed.2, 10);
            assert!(initial["retained"]
                .as_array()
                .unwrap()
                .contains(&json!(expected)));
        }
        for (value, expected, marker) in [(3, &third, 20), (2, &second, 30), (1, &first, 40)] {
            resolve(&mut state, value, true);
            let done = poll(&mut state, marker);
            assert_eq!(
                done["completed"],
                json!([{"id":expected,"ok":true,"returned":true,"value":{"$bigint":value.to_string()}}])
            );
            assert!(state.seen.contains(&(0, expected.parse().unwrap(), marker)));
            assert_eq!(done["retired"], false);
        }
        let idle = poll(&mut state, 50);
        assert_eq!(idle["active"], false);
        assert_eq!(idle["retained"], json!([]));
        assert_eq!(idle["completed"], json!([]), "replies are delivered once");
        finish(&mut state);
    }
}
#[test]
fn ffi_driver_keeps_post_return_context_and_reports_background_import_failure() {
    unsafe {
        let mut state = State::default();
        fixture(&mut state, true);
        let id = submit(state.host, call(9)).unwrap();
        let done = poll(&mut state, 10);
        assert_eq!(done["completed"][0]["id"], id);
        assert_eq!(done["active"], true);
        assert_eq!(done["retained"], json!([id]));
        resolve(&mut state, 9, true);
        let idle = poll(&mut state, 20);
        assert!(state.seen.contains(&(0, id.parse().unwrap(), 20)));
        assert_eq!(idle["retained"], json!([]));
        let id = submit(state.host, call(8)).unwrap();
        assert_eq!(poll(&mut state, 30)["completed"][0]["id"], id);
        resolve(&mut state, 8, false);
        let failed = poll(&mut state, 40);
        assert_eq!(failed["retired"], true);
        assert_eq!(failed["failedOrigins"], json!([id]));
        assert!(failed["error"]
            .as_str()
            .unwrap()
            .contains("expected rejected import"));
        assert_eq!(failed["retained"], json!([]));
        assert!(submit(state.host, call(7)).is_err());
        finish(&mut state);
    }
}
#[test]
fn ffi_driver_cancellation_distinguishes_queued_from_started_calls() {
    unsafe {
        let mut state = State::default();
        fixture(&mut state, false);
        let id = submit(state.host, call(5)).unwrap();
        let mut out = Buffer::default();
        let status = perry_component_driver_cancel(state.host, id.parse().unwrap(), &mut out);
        reply(status, out).unwrap();
        assert_eq!(poll(&mut state, 10)["completed"], json!([]));
        assert!(state.seen.is_empty());
        let id = submit(state.host, call(6)).unwrap();
        assert_eq!(poll(&mut state, 20)["active"], true);
        let mut out = Buffer::default();
        let status = perry_component_driver_cancel(state.host, id.parse().unwrap(), &mut out);
        reply(status, out).unwrap();
        assert_eq!(poll(&mut state, 30)["retired"], true);
        assert!(submit(state.host, call(7)).is_err());
        finish(&mut state);
    }
}
const SCALAR_WORLD: &str = "package test:factory; world plugin { export data: func(value: future<tuple<u64,string,list<u8>>>) -> future<tuple<u64,string,list<u8>>>; export strings: func(value: stream<string>) -> stream<string>; export void: func(value: future) -> future; export units: func(value: stream) -> stream; export nested: func(value: future<future<u32>>) -> future<future<u32>>; }";
unsafe fn completed(state: &mut State, request: Value) -> Value {
    let id = submit(state.host, request).unwrap();
    for _ in 0..128 {
        let results = poll(state, 100);
        if let Some(event) = results["completed"].as_array().unwrap().first() {
            assert_eq!(event["id"], id);
            assert_eq!(event["ok"], true, "{event}");
            return event["value"].clone();
        }
    }
    panic!("command did not finish");
}
fn payload_kind(schema: &Value, world: &perry_wit::World) -> (&'static str, &'static str) {
    use perry_wit::Type;
    let schema: Type = serde_json::from_value(schema.clone()).unwrap();
    let (kind, mut payload) = match &schema {
        Type::Future(Some(t)) => ("future", t.as_ref()),
        Type::Stream(Some(t)) => ("stream", t.as_ref()),
        _ => return ("other", "void"),
    };
    while let Type::Ref(key) = payload {
        payload = &world.types[key].ty;
    }
    (
        kind,
        match payload {
            Type::Tuple(_) => "tuple",
            Type::String => "string",
            _ => "other",
        },
    )
}
#[test]
fn ffi_driver_transports_compound_factories_and_partial_stream_writes() {
    unsafe {
        let mut state = State::default();
        new(
            &mut state,
            SCALAR_WORLD,
            include_str!("../../async_values/factory-echo.component.wat"),
        );
        let mut out = Buffer::default();
        let status = perry_component_driver_types(state.host, &mut out);
        let types = reply(status, out).unwrap();
        assert_eq!(types.as_array().unwrap().len(), 6);
        // Find schemas by the maintained WIT representation rather than
        // assuming the private composition adapter order.
        let world = perry_wit::parse(SCALAR_WORLD, None).unwrap();
        let future = types
            .as_array()
            .unwrap()
            .iter()
            .find(|t| payload_kind(&t["schema"], &world) == ("future", "tuple"))
            .unwrap()["index"]
            .clone();
        let pair = completed(&mut state, json!({"op":"create","index":future}));
        let write = submit(state.host,json!({"op":"write","writer":pair["writer"],"values":[[{"$bigint":"18446744073709551615"},"héllo 🌍",[0,255,33]]]})).unwrap();
        assert_eq!(poll(&mut state, 1)["completed"], json!([]));
        let read = submit(state.host, json!({"op":"read","value":pair["value"]})).unwrap();
        let result = poll(&mut state, 2);
        let events = result["completed"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        let written = events.iter().find(|v| v["id"] == write).unwrap();
        assert_eq!(written["value"]["written"], 1);
        let received = events.iter().find(|v| v["id"] == read).unwrap();
        assert_eq!(
            received["value"],
            json!([{"$bigint":"18446744073709551615"},"héllo 🌍",[0,255,33]])
        );
        let stream = types
            .as_array()
            .unwrap()
            .iter()
            .find(|t| payload_kind(&t["schema"], &world) == ("stream", "string"))
            .unwrap()["index"]
            .clone();
        let pair = completed(&mut state, json!({"op":"create","index":stream}));
        let write = submit(
            state.host,
            json!({"op":"write","writer":pair["writer"],"values":["one","two","three"]}),
        )
        .unwrap();
        let read = submit(
            state.host,
            json!({"op":"read","value":pair["value"],"capacity":1}),
        )
        .unwrap();
        let done = poll(&mut state, 3);
        let events = done["completed"].as_array().unwrap();
        assert_eq!(
            events.iter().find(|v| v["id"] == read).unwrap()["value"],
            json!({"done":false,"value":["one"]})
        );
        assert_eq!(
            events.iter().find(|v| v["id"] == write).unwrap()["value"]["remaining"],
            json!(["two", "three"])
        );
        completed(
            &mut state,
            json!({"op":"close-writer","writer":pair["writer"]}),
        );
        assert_eq!(
            completed(&mut state, json!({"op":"read","value":pair["value"]})),
            json!({"done":true})
        );
        finish(&mut state);
    }
}

unsafe extern "C" fn resource_callback(
    context: u64,
    origin: u64,
    event: *const u8,
    len: usize,
    out: *mut Buffer,
) -> i32 {
    let frame = &*(context as usize as *const Frame);
    let state = &mut *frame.state;
    let event: Value = serde_json::from_slice(bytes(event, len).unwrap()).unwrap();
    let reject = event["kind"] == "drop" && state.reject_drop;
    state.resources.push((event, origin, frame.marker));
    if reject {
        output(out, b"secondary destructor error".to_vec());
        ERROR
    } else {
        output(out, b"[]".to_vec());
        OK
    }
}
#[test]
fn ffi_driver_routes_retirement_to_cleanup_frame_without_replacing_primary_error() {
    unsafe {
        let mut state = State::default();
        new(
            &mut state,
            crate::host_resource_tests::WORLD,
            crate::host_resource_tests::COMPONENT,
        );
        let world = perry_wit::parse(crate::host_resource_tests::WORLD, None).unwrap();
        let key = &world.imports[0].resources["token"];
        let owned = |name: &str| json!({"$hostResource":name,"type":key,"generation":1,"owned":true,"new":true});
        // Keep one payload in the guest so failure retirement must run its
        // destructor, after the primary import has already rejected.
        completed(
            &mut state,
            json!({"op":"call","interface":"","function":"keep","args":[owned("held")]}),
        );
        state.reject_drop = true;
        let id=submit(state.host,json!({"op":"call","interface":"","function":"consume-imported","args":[owned("primary")]})).unwrap();
        let failed = poll(&mut state, 70);
        assert_eq!(failed["retired"], true);
        assert!(failed["completed"][0]["error"]
            .as_str()
            .unwrap()
            .contains("primary synchronous import error"));
        assert_eq!(failed["failedOrigins"], json!([id, "0"]));
        let drops: Vec<_> = state
            .resources
            .iter()
            .filter(|(event, _, _)| event["kind"] == "drop")
            .collect();
        assert!(!drops.is_empty());
        assert!(drops
            .iter()
            .all(|(_, origin, marker)| *origin == 0 && *marker == 1070));
        assert_eq!(
            poll(&mut state, 80)["failedOrigins"],
            json!([]),
            "failure notifications are consumed once"
        );
        finish(&mut state);
    }
}
#[test]
fn ffi_driver_preflight_quota_null_output_and_mode_guards_preserve_queued_work() {
    unsafe {
        let mut state = State::default();
        fixture(&mut state, false);
        let id = submit(state.host, call(4)).unwrap();
        assert_eq!(
            perry_component_driver_poll(state.host, 0, 0, Some(wake), 1, std::ptr::null_mut()),
            ERROR
        );
        assert!(
            state.seen.is_empty(),
            "null output must not consume replies or run guest code"
        );
        assert!(submit(state.host, json!({"op":"create","index":99999})).is_err());
        assert!(submit(
            state.host,
            json!({"op":"call","interface":"","function":"run","args":[],"unknown":1})
        )
        .is_err());
        let mut out = Buffer::default();
        let status = perry_component_host_call(
            state.host,
            b"".as_ptr(),
            0,
            b"run".as_ptr(),
            3,
            b"[]".as_ptr(),
            2,
            0,
            &mut out,
        );
        assert!(reply(status, out)
            .unwrap_err()
            .to_string()
            .contains("legacy"));
        assert_eq!(poll(&mut state, 10)["retired"], false);
        resolve(&mut state, 4, true);
        assert_eq!(poll(&mut state, 20)["completed"][0]["id"], id);
        for _ in 0..64 {
            submit(
                state.host,
                json!({"op":"call","interface":"","function":"missing","args":[]}),
            )
            .unwrap();
        }
        assert!(submit(state.host, call(5))
            .unwrap_err()
            .to_string()
            .contains("too many pending"));
        let done = poll(&mut state, 30);
        assert_eq!(done["completed"].as_array().unwrap().len(), 64);
        assert_eq!(done["retired"], false);
        let id = submit(state.host, call(6)).unwrap();
        poll(&mut state, 40);
        resolve(&mut state, 6, true);
        assert_eq!(poll(&mut state, 50)["completed"][0]["id"], id);
        finish(&mut state);
    }
}

#[test]
fn ffi_driver_explicit_disposal_reports_released_error_and_native_drop_calls_no_embedding() {
    for explicit in [false, true] {
        unsafe {
            let mut state = State::default();
            new(
                &mut state,
                crate::host_resource_tests::WORLD,
                crate::host_resource_tests::COMPONENT,
            );
            let world = perry_wit::parse(crate::host_resource_tests::WORLD, None).unwrap();
            let key = &world.imports[0].resources["token"];
            let value =
                json!({"$hostResource":"held","type":key,"generation":1,"owned":true,"new":true});
            completed(
                &mut state,
                json!({"op":"call","interface":"","function":"keep","args":[value]}),
            );
            state.reject_drop = true;
            let before = state.resources.len();
            if explicit {
                let frame = Frame {
                    state: &mut state,
                    marker: 500,
                };
                let mut out = Buffer::default();
                let status = perry_component_host_dispose(
                    state.host,
                    &frame as *const Frame as usize as u64,
                    &mut out,
                );
                assert_eq!(status, RELEASED_ERROR);
                perry_component_host_buffer_drop(out);
                let drops: Vec<_> = state.resources[before..]
                    .iter()
                    .filter(|(e, _, _)| e["kind"] == "drop")
                    .collect();
                assert_eq!(drops.len(), 1);
                assert_eq!(drops[0].2, 500);
                assert!(submit(
                    state.host,
                    json!({"op":"call","interface":"","function":"keep","args":[]})
                )
                .is_err());
                finish(&mut state);
            } else {
                perry_component_host_drop(state.host);
                assert_eq!(
                    state.resources.len(),
                    before,
                    "native finalization must not call embedding code"
                );
            }
        }
    }
}
