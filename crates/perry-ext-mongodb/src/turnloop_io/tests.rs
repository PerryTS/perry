//! Unit tests for the turnloop MongoDB transport.
//!
//! These run entirely offline: the operation-queue test drives a real
//! `turnloop_mongodb::Connection` through a synthetic handshake and synthetic
//! replies, so it exercises the actual state machine rather than a mock of it.
//! Nothing here opens a socket, and nothing here has ever talked to a real
//! `mongod` — see the module docs for what that leaves unverified.

use bson::doc;
use bson::raw::RawDocumentBuf;
use perry_db_turnloop::{DbCore, TlsFacts};
use perry_ffi::JsPromise;
use turnloop_mongodb::wire::{self, Message, DEFAULT_MAX_MESSAGE};

use super::connection::MongoCore;
use super::ops::Operation;
use super::*;

/// Encode one OP_MSG reply to `response_to`, the way a server would.
fn reply_frame(response_to: i32, body: bson::Document) -> Vec<u8> {
    let raw = RawDocumentBuf::try_from(&body).expect("test reply must encode");
    let mut out = Vec::new();
    wire::encode(&mut out, 1, response_to, 0, &raw, &[], DEFAULT_MAX_MESSAGE)
        .expect("test reply must frame");
    out
}

/// A minimal `hello` response. `maxWireVersion` has to be at least 6 or the
/// connection refuses the server as too old.
fn hello_frame(response_to: i32) -> Vec<u8> {
    reply_frame(
        response_to,
        doc! {
            "ok": 1.0,
            "helloOk": true,
            "isWritablePrimary": true,
            "minWireVersion": 0i32,
            "maxWireVersion": 25i32,
        },
    )
}

/// An exhausted cursor with no rows — the shape `findOne` gets for a filter
/// that matched nothing.
fn empty_cursor_frame(response_to: i32, namespace: &str) -> Vec<u8> {
    reply_frame(
        response_to,
        doc! { "ok": 1.0, "cursor": { "id": 0i64, "ns": namespace, "firstBatch": [] } },
    )
}

/// The request id inside an encoded OP_MSG, which is what a reply must name in
/// its `responseTo` for the connection to accept it.
fn request_id_of(frame: &[u8]) -> i32 {
    i32::from_le_bytes(
        frame[4..8]
            .try_into()
            .expect("a framed message has a header"),
    )
}

#[test]
fn the_subsystem_slot_is_the_one_reserved_for_this_binding() {
    // Each P7 binding links as its own `staticlib` with its own sink function,
    // so two bindings sharing a slot would route one's completions into the
    // other's connection table.
    assert_eq!(SUBSYSTEM, subsystem::MONGODB);
    assert_ne!(SUBSYSTEM, subsystem::PG);
    assert_ne!(SUBSYSTEM, subsystem::MYSQL);
    assert_ne!(SUBSYSTEM, subsystem::REDIS);
}

#[test]
fn registration_passes_the_abi_layout_check() {
    // The dev-dependency links the runtime, so this exercises the real
    // `register_sink`: a mismatch between perry-ffi's `NetCompletion` layout
    // digest and the runtime's refuses registration, leaves `available` false,
    // and would silently put every client back on the legacy transport. On an
    // agent with no loop — a `worker_threads` Worker — this is false and that
    // fallback is the correct behaviour.
    assert!(
        super::register_only(),
        "a false here is an ABI layout mismatch between perry-ffi and perry-runtime"
    );
    assert!(perry_ffi::turnloop_net::sink_installed(SUBSYSTEM));
}

#[test]
fn a_plain_single_host_uri_is_accepted_and_keeps_the_five_second_connect_timeout() {
    // The accept case. The timeout matters on its own: `turnloop_mongodb`
    // defaults to 30 s where the pre-P7 path installed 5 s, so a dead host
    // would otherwise take six times longer to report itself than today.
    let endpoint = classify_uri("mongodb://127.0.0.1:27017/app")
        .expect("a direct plaintext single-host URI is exactly what this slice migrates");
    assert_eq!(endpoint.host, "127.0.0.1");
    assert_eq!(endpoint.port, 27017);
    assert_eq!(endpoint.options.connect_timeout, LEGACY_CONNECT_TIMEOUT);
    assert_eq!(endpoint.options.database.as_deref(), Some("app"));

    // An explicit connectTimeoutMS is the caller's, not ours to override.
    let explicit = classify_uri("mongodb://127.0.0.1:27017/app?connectTimeoutMS=1500")
        .expect("an explicit connect timeout is still in scope");
    assert_eq!(
        explicit.options.connect_timeout,
        std::time::Duration::from_millis(1500)
    );
}

#[test]
fn every_out_of_scope_uri_declines_to_the_legacy_transport() {
    // Each of these names a subsystem this slice does not implement. Declining
    // keeps the existing `mongodb`-crate path — with its SRV resolution, TLS,
    // topology monitors and pool — rather than connecting one plaintext socket
    // to a seed and calling it a replica set.
    for uri in [
        // SRV: needs DNS SRV + TXT resolution before there is an address.
        "mongodb+srv://cluster.example.com/app",
        // A per-connection trust store or verification override. `tls=true`
        // itself is in scope now; these keys are not, and `Options::parse` is
        // what refuses them — this path honours the process TLS environment
        // and has nowhere to put a trust store belonging to one URI.
        "mongodb://127.0.0.1:27017/app?tls=true&tlsCAFile=/etc/ca.pem",
        "mongodb://127.0.0.1:27017/app?tls=true&tlsInsecure=true",
        "mongodb://127.0.0.1:27017/app?tls=true&tlsAllowInvalidCertificates=true",
        // Several hosts: server selection.
        "mongodb://a.example.com:27017,b.example.com:27017/app",
        // A replica set: topology discovery and primary election.
        "mongodb://127.0.0.1:27017/app?replicaSet=rs0",
        // Compression is deliberately left off, so a URI asking for it must not
        // silently get an uncompressed connection.
        "mongodb://127.0.0.1:27017/app?compressors=zlib",
        // Unparseable, and an unknown option this parser rejects: the legacy
        // path produces its own `Failed to parse URI` rejection, so the error a
        // bad URI gets does not depend on which parser saw it first.
        "postgres://127.0.0.1:5432/app",
        "mongodb://127.0.0.1:27017/app?notARealOption=1",
    ] {
        assert!(
            classify_uri(uri).is_none(),
            "{uri} is out of this slice's scope and must keep the legacy path"
        );
    }
}

#[test]
fn a_tls_uri_is_in_scope_and_its_options_name_the_seed_host() {
    // The decline this replaces was the reason a `tls=true` URI never reached
    // this transport at all, however plain its topology.
    let endpoint = classify_uri("mongodb://db.example.com:27017/app?tls=true")
        .expect("a direct single-host TLS URI is in scope now");
    assert_eq!(endpoint.host, "db.example.com");
    assert!(
        endpoint.options.tls,
        "the core is what raises UpgradeTls; a false here never asks"
    );

    let options = tls_options(&endpoint).expect("a TLS URI configures the driver");
    assert_eq!(
        options.servername, "db.example.com",
        "the certificate is checked against the host the URI named"
    );
    assert!(
        options.alpn.is_empty(),
        "OP_MSG is the only protocol on this socket, so nothing is offered"
    );
    // `reject_unauthorized` and the trust roots are deliberately not asserted:
    // they are whatever the process TLS environment says, which is the point of
    // building them with `from_node_environment`.

    // `ssl=true` is the same option under its older name.
    let aliased = classify_uri("mongodb://db.example.com:27017/app?ssl=true")
        .expect("ssl= is tls= under its older name");
    assert!(tls_options(&aliased).is_some());
}

#[test]
fn a_plaintext_uri_carries_no_tls_options() {
    // Not merely unused. `None` is what makes an `UpgradeTls` on a plaintext
    // connection a driver error instead of a silent plaintext continuation, so
    // the URI and the options must agree.
    let endpoint =
        classify_uri("mongodb://127.0.0.1:27017/app").expect("the plain case stays in scope");
    assert!(!endpoint.options.tls);
    assert!(tls_options(&endpoint).is_none());
}

#[test]
fn a_tls_core_holds_the_hello_until_the_upgrade_is_acknowledged() {
    // Driven through the real state machine. The handshake carries the client
    // metadata and, on a URI with credentials, the speculative SCRAM — so a
    // byte produced before the session exists is a byte sent in the clear: the
    // driver flushes the core's output *before* it installs anything.
    let endpoint = classify_uri("mongodb://db.example.com:27017/app?tls=true")
        .expect("a direct single-host TLS URI is in scope now");
    let mut core = MongoCore::new(endpoint.options, "0123456789abcdefghij".to_string());

    core.transport_connected().expect("the transport came up");
    assert!(
        core.output().is_empty(),
        "the hello must not precede the upgrade"
    );
    assert_eq!(
        core.drain(),
        Ok(false),
        "an upgrade request is not a terminal event"
    );
    assert!(core.take_tls_request(), "the core asked for the upgrade");
    assert!(
        !core.take_tls_request(),
        "the request is taken once, or the driver installs a second session"
    );

    core.tls_established(&TlsFacts::default())
        .expect("the core accepts the acknowledgement in its TLS state");
    assert!(
        String::from_utf8_lossy(core.output()).contains("isMaster"),
        "the hello goes out after the upgrade, encrypted by the session"
    );
}

#[test]
fn the_queue_issues_in_submission_order_and_loses_nothing() {
    // The single most important property in this module. MongoDB's wire
    // protocol is request/response turn-taking and the core refuses a second
    // command while one is outstanding ("Connection busy"), but JavaScript has
    // no such rule: `Promise.all([find(a), find(b)])` submits both before
    // either resolves. A second submission that were dropped, reordered or
    // rejected would only show up under concurrency.
    let endpoint =
        classify_uri("mongodb://127.0.0.1:27017/app").expect("the accept case is tested above");
    let mut core = MongoCore::new(endpoint.options, "0123456789abcdefghij".to_string());

    core.transport_connected()
        .expect("a plaintext connection handshakes immediately");
    let handshake = core.output().to_vec();
    assert!(
        !handshake.is_empty(),
        "the connection must emit its hello before anything else"
    );
    let handshake_id = request_id_of(&handshake);
    core.consume_output(handshake.len());

    // Three operations submitted before the handshake finished. All three must
    // survive; none may reach the wire yet.
    let collections = ["alpha", "beta", "gamma"];
    for collection in collections {
        let (operation, request) = Operation::find_one("app", collection, "{}")
            .expect("a findOne on an empty filter always builds");
        core.submit(operation, request, JsPromise::new());
    }
    assert_eq!(
        core.queued(),
        3,
        "an operation submitted before the connection is ready must queue, not fail"
    );
    assert!(
        core.output().is_empty(),
        "nothing may be issued before the handshake reply arrives"
    );

    core.receive(&hello_frame(handshake_id))
        .expect("a hello reply is accepted while the handshake is outstanding");
    assert!(
        !core
            .drain()
            .expect("a successful handshake is not a failure"),
        "the connection is not finished after a successful handshake"
    );

    // Now they come out, one at a time, in submission order.
    for (index, expected) in collections.iter().enumerate() {
        assert_eq!(
            core.queued(),
            collections.len() - index - 1,
            "exactly one operation is outstanding at a time"
        );
        let frame = core.output().to_vec();
        assert!(
            !frame.is_empty(),
            "operation {expected} should have been issued by now"
        );
        let message = Message::parse(&frame, DEFAULT_MAX_MESSAGE).expect("a valid OP_MSG");
        assert_eq!(
            message.body.get_str("find").expect("a find command"),
            *expected,
            "operations must reach the wire in submission order"
        );
        let issued_id = message.request_id;
        core.consume_output(frame.len());
        assert!(
            core.has_pending_work(),
            "an outstanding operation is work the process owes an answer for"
        );

        core.receive(&empty_cursor_frame(issued_id, &format!("app.{expected}")))
            .expect("a reply is accepted while a command is outstanding");
        assert!(
            !core.drain().expect("an ordinary reply is not a failure"),
            "the connection stays open between operations"
        );
    }

    assert_eq!(core.queued(), 0, "every submission was answered");
    assert!(
        !core.has_pending_work(),
        "an idle connection must not keep the process alive"
    );
}

#[test]
fn a_reply_that_arrives_split_across_reads_is_reassembled() {
    // A socket read does not respect frame boundaries, and the core's `receive`
    // deliberately stops at one — it returns the consumed prefix and refuses to
    // be fed while it holds an unreleased reply. Dropping the remainder would
    // hang the next operation on bytes that already arrived.
    let endpoint =
        classify_uri("mongodb://127.0.0.1:27017/app").expect("the accept case is tested above");
    let mut core = MongoCore::new(endpoint.options, "0123456789abcdefghij".to_string());
    core.transport_connected().expect("plaintext handshake");
    let handshake = core.output().to_vec();
    let handshake_id = request_id_of(&handshake);
    core.consume_output(handshake.len());

    let frame = hello_frame(handshake_id);
    let (head, tail) = frame.split_at(frame.len() / 2);
    core.receive(head).expect("a partial frame is staged");
    assert!(!core.drain().expect("a partial frame is not a failure"));

    let (operation, request) = Operation::find_one("app", "alpha", "{}").expect("findOne builds");
    core.submit(operation, request, JsPromise::new());
    assert_eq!(
        core.queued(),
        1,
        "the handshake is still outstanding, so nothing may be issued"
    );

    core.receive(tail).expect("the rest of the frame is staged");
    assert!(!core
        .drain()
        .expect("the completed handshake is not a failure"));
    assert_eq!(
        core.queued(),
        0,
        "the reassembled handshake reply must release the queued operation"
    );
    assert!(!core.output().is_empty(), "and put it on the wire");
}

#[test]
fn a_generated_id_is_prepended_and_a_caller_supplied_id_is_kept() {
    // Both halves are observable. The `mongodb` crate's
    // `get_or_prepend_id_field` puts a generated `_id` first, and BSON field
    // order survives a round trip through the server, so appending would make
    // documents written on this transport differ from documents written on the
    // legacy one. The returned id is what `insertOne` resolves with.
    let (document, id) = super::ops::insert_document(doc! { "name": "ada", "year": 1815i32 })
        .expect("a document with no _id gets a generated one");
    assert_eq!(
        document.keys().next().map(String::as_str),
        Some("_id"),
        "a generated _id must come first"
    );
    assert!(
        id.to_string().starts_with("ObjectId(\""),
        "insertOne resolves the id's shell rendering, as the pre-P7 path did: {id}"
    );

    let (kept, id) = super::ops::insert_document(doc! { "_id": 7i32, "name": "ada" })
        .expect("a caller-supplied _id is left alone");
    assert_eq!(kept.get_i32("_id").expect("the caller's own _id"), 7);
    assert_eq!(id.to_string(), "7");
}

#[test]
fn a_client_nonce_is_unpredictable_and_satisfies_scram_s_own_rule() {
    // `Scram::new` rejects a nonce under 16 bytes or containing anything
    // outside printable ASCII, comma excluded — it is part of the SCRAM
    // message's own grammar. A nonce that failed this would turn every
    // authenticated connection into a handshake error.
    let first = client_nonce().expect("this host has /dev/urandom");
    let second = client_nonce().expect("this host has /dev/urandom");
    assert!(first.len() >= 16, "SCRAM requires at least 16 bytes");
    assert!(
        first
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b','),
        "every byte must be printable and not a comma: {first}"
    );
    assert_ne!(
        first, second,
        "two nonces from a CSPRNG must not repeat; a fixed nonce would break \
         SCRAM's replay resistance"
    );
}
