//! Acceptance for the axios surface's transport and result shape.
//!
//! P8's inventory recorded this surface as taking `reqwest` *unconditionally* —
//! it never asked the turnloop engine at all. These tests' subject is the
//! asking, so each one watches a counter the engine itself bumps rather than
//! settling for "nothing threw".

use std::sync::atomic::{AtomicUsize, Ordering};

use super::{AxiosRequest, AxiosResponseHandle, POINTER_TAG};
use crate::turnloop_client::{self, Outcome, Sink};

/// A body always goes out as JSON, and a bodyless verb must not invent a
/// `Content-Type` — the shape the reqwest client this replaced put on the wire.
#[test]
fn a_json_body_carries_its_content_type_and_a_bodyless_verb_carries_none() {
    let with_body = AxiosRequest {
        url: "http://example.test/x".into(),
        method: "POST",
        body: Some("{\"a\":1}".into()),
    };
    assert_eq!(
        with_body.headers(),
        vec![("Content-Type".to_string(), "application/json".to_string())]
    );

    let without = AxiosRequest {
        url: "http://example.test/x".into(),
        method: "GET",
        body: None,
    };
    assert!(
        without.headers().is_empty(),
        "a GET must not carry a body's content type"
    );
}

/// #340: the handle both transports hand back must be NaN-boxed as an object,
/// or the awaiter sees a subnormal float and every `r.status` / `r.data` decays
/// to `undefined`. Asserts the round trip, not just the tag.
#[test]
fn a_registered_response_comes_back_through_its_nan_boxed_handle() {
    let bits = super::store(
        207,
        "Multi-Status".to_string(),
        "{\"ok\":true}".to_string(),
        vec![("content-type".into(), "application/json".into())],
    );
    assert_eq!(
        bits & 0xFFFF_0000_0000_0000,
        POINTER_TAG,
        "the handle must be boxed as an object"
    );
    let handle = (bits & 0x0000_FFFF_FFFF_FFFF) as crate::common::Handle;
    let response = crate::common::get_handle::<AxiosResponseHandle>(handle)
        .expect("the boxed handle must address the registered response");
    assert_eq!(response.status, 207);
    assert_eq!(response.status_text, "Multi-Status");
    assert_eq!(response.data, "{\"ok\":true}");
}

static SINK_CALLS: AtomicUsize = AtomicUsize::new(0);

fn count_only(_ctx: usize, _outcome: Outcome) {
    SINK_CALLS.fetch_add(1, Ordering::SeqCst);
}

/// axios rides the SAME engine `fetch` does.
///
/// `submitted_total()` is bumped by the engine's `start_here` — the point where
/// a request is entered into the per-agent table and started — so watching it
/// move is what separates "axios offered the request to the engine" from "the
/// call returned". Before this lane it could not move for an axios request at
/// all: the surface went straight to `reqwest::Client::new()`.
///
/// The target is a closed loopback port. The subject is the routing, not the
/// response.
#[test]
fn an_axios_request_rides_the_engine_fetch_uses() {
    let _lease = turnloop_client::become_the_owner_for_test();
    let before = turnloop_client::submitted_total();
    let request = AxiosRequest {
        url: "http://127.0.0.1:1/axios".into(),
        method: "GET",
        body: None,
    };
    assert!(
        super::submit_with(
            &request,
            Sink {
                ctx: 0,
                on_head: None,
                on_chunk: None,
                on_done: count_only,
            },
        ),
        "the engine must accept a plain http GET; a decline here would send \
         every axios call back to reqwest"
    );
    assert!(
        turnloop_client::submitted_total() > before,
        "the engine must have STARTED the request — without this the call could \
         have been accepted and done nothing"
    );
}
