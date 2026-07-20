//! Drift guard for the stub-elimination epic (#4919 / keystone #4918).
//!
//! The manifest's `ApiEntry.stub` flag is the single source of truth for
//! "this API is registered and callable, but no-op / fake / silently
//! wrong / partial". Before #4918 the flag existed but was never set, so
//! every doc surface (`.d.ts`, `reference.md`, `--print-api-manifest`)
//! presented stubs as fully supported.
//!
//! These tests pin the exact stub inventory so a new lie cannot ship
//! silently: adding or removing a `.stub()`/`.stub_note()` annotation
//! moves a number here, forcing a reviewed, deliberate change — the same
//! discipline `manifest_consistency.rs` applies to dispatch-table arity.

use perry_api_manifest::iter_entries;

/// Parse the trailing `(#1234)` issue tag from a stub note.
fn issue_tag(note: &str) -> Option<String> {
    let start = note.rfind("(#")?;
    let end = note[start..].find(')')? + start;
    Some(note[start + 1..end].to_string())
}

#[test]
fn every_stub_has_a_documented_reason() {
    // Policy: a stub may exist, but it must be *visible* — a human note
    // explaining how it lies, tagged with the tracking issue. No bare
    // `.stub()` without a reason is allowed in the table.
    for e in iter_entries().filter(|e| e.stub) {
        let note = e.stub_note.unwrap_or_else(|| {
            panic!(
                "stub entry {}::{} has no stub_note — use .stub_note(\"reason (#issue)\")",
                e.module, e.name
            )
        });
        assert!(
            issue_tag(note).is_some(),
            "stub note for {}::{} must carry a tracking issue tag like (#4911): {:?}",
            e.module,
            e.name,
            note
        );
    }
}

#[test]
fn stub_inventory_matches_known_clusters() {
    use std::collections::BTreeMap;
    let mut by_issue: BTreeMap<String, usize> = BTreeMap::new();
    for e in iter_entries().filter(|e| e.stub) {
        if let Some(note) = e.stub_note {
            if let Some(tag) = issue_tag(note) {
                *by_issue.entry(tag).or_default() += 1;
            }
        }
    }

    // Expected per-issue stub counts. Update these (and the changelog)
    // when a stub is implemented for real (count goes down) or a newly
    // discovered lie is annotated (count goes up). A surprise diff here
    // means an API silently changed honesty state.
    let expected: &[(&str, usize)] = &[
        // #4911 (dns/dns/promises resolution + dgram UDP) intentionally absent:
        // real `getaddrinfo`/DNS/UDP I/O landed; the in-process loopback mode is
        // now opt-in behind `PERRY_DETERMINISTIC_NET=1`, not a silent stub.
        // #4912 (child_process.spawn) intentionally absent: spawn is real
        // since #1780 (Expr::ChildProcessSpawn codegen). The audit's
        // "returns null" premise was stale; only exec() timing remains.
        // #4914 (cluster port sharing) intentionally absent: workers share a
        // listening port via SO_REUSEPORT binds + a fork-IPC 'listening'
        // round-trip; SCHED_RR fd-passing + shared ephemeral `listen(0)`
        // port are tracked in #4962 and are policy fidelity, not lies.
        // #4915 (BYOB readers + ByteLengthQueuingStrategy) intentionally
        // absent: read(view), controller.byobRequest respond/
        // respondWithNewView, and real byteLength desiredSize accounting
        // landed (perry-stdlib/src/streams/byob.rs).
        // #4916 (v8 get/writeHeapSnapshot) heap snapshots intentionally
        // absent: real object graph from the GC heap walk landed
        // (perry-runtime/src/gc/heap_snapshot.rs). The remaining #4916
        // entries are documented approximations/fakes from the same
        // diagnostics audit: v8 heap-stats provenance(3) +
        // GCProfiler.stop empty statistics(1) + inspector
        // open/url/waitForDebugger/Session.post(4) + repl
        // start/REPLServer no-eval-loop(2).
        ("#4916", 10),
        // #4917 (stdlib-adapter no-ops) — what remains after the real
        // semantics landed: zlib deflate-family compressor factories
        // honor `level` but still drop strategy/memLevel (3) +
        // Brotli/zstd factories ignore `params`, warn once (4) +
        // http.Agent per-socket keepSocketAlive/reuseSocket hooks (2).
        // Intentionally absent now: zlib decompressor factories (level
        // honored; a missing dictionary fails loudly), Agent.destroy
        // (drops the per-agent reqwest pool), worker ref/unref (real
        // event-loop refcount), mongodb.findOne (parsed document),
        // exponential-backoff options (honored, incl. retry predicate).
        ("#4917", 9),
        // #6562 (bun:ffi stage 1) — the stage-≥2 FFI surface is declared so
        // feature probes get a clear error, but throws at runtime until the
        // later stages land: toArrayBuffer, toBuffer, JSCallback, CFunction,
        // linkSymbols, viewSource, read.
        ("#6562", 7),
    ];
    let expected_map: BTreeMap<String, usize> =
        expected.iter().map(|(k, v)| (k.to_string(), *v)).collect();

    assert_eq!(
        by_issue, expected_map,
        "stub inventory drifted — a stub was added/removed without updating tests/stub_inventory.rs"
    );
}

#[test]
fn stubs_only_appear_in_allowlisted_modules() {
    // A stub flag showing up on a module not in this list is almost
    // certainly an accident — fail loud so it gets triaged.
    let allowed = [
        "stream/web",
        "streams",
        "v8",
        "zlib",
        "http",
        "https",
        "worker_threads",
        "mongodb",
        "exponential-backoff",
        "inspector",
        "repl",
        // #6562: bun:ffi stage-≥2 exports are declared-but-throwing stubs.
        "bun:ffi",
    ];
    for e in iter_entries().filter(|e| e.stub) {
        assert!(
            allowed.contains(&e.module),
            "unexpected stub on module {:?} ({}). If intentional, add it to the allowlist.",
            e.module,
            e.name
        );
    }
}

#[test]
fn keystone_apis_are_flagged() {
    // Spot-check the headline lies from the epic so a refactor can't
    // quietly drop the flag on the worst offenders.
    let must_be_stub: &[(&str, &str)] = &[
        ("repl", "start"),
        ("inspector", "post"),
        // still partial after #4917: level honored, strategy/memLevel dropped
        ("zlib", "createGzip"),
        ("zlib", "createBrotliCompress"),
        ("http", "keepSocketAlive"),
    ];
    for (module, name) in must_be_stub {
        let found = iter_entries().any(|e| e.module == *module && e.name == *name && e.stub);
        assert!(found, "expected {}::{} to be flagged stub", module, name);
    }

    // The inverse: APIs implemented for real must NOT stay flagged.
    // v8 heap snapshots emit a real GC-walk object graph since #4916.
    // mongodb.findOne resolves a parsed document, backOff honors its
    // options, and worker ref/unref drive the event-loop refcount since
    // #4917.
    let must_not_be_stub: &[(&str, &str)] = &[
        ("v8", "getHeapSnapshot"),
        ("v8", "writeHeapSnapshot"),
        ("mongodb", "findOne"),
        ("exponential-backoff", "backOff"),
        ("worker_threads", "ref"),
        ("worker_threads", "unref"),
        ("http", "destroy"),
    ];
    for (module, name) in must_not_be_stub {
        let flagged = iter_entries().any(|e| e.module == *module && e.name == *name && e.stub);
        assert!(
            !flagged,
            "{}::{} is implemented for real and must not be flagged stub",
            module, name
        );
    }
}
