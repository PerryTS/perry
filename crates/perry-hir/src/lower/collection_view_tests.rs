//! Verdict tests for [`super::for_head::rewrite_collection_view_for_of`].
//!
//! These assert **which lowering a loop got**, not what it computes. The
//! iterator-object path is a correct fallback, so a test that only compared
//! program output would stay green if the fast path silently stopped
//! applying — CLAUDE.md's fourth way a gate can be unable to fail. Behaviour
//! is covered separately by the `.ts` semantics matrix
//! (`test-files/test_gap_map_view_for_of.ts`, byte-compared against node).
//!
//! The verdict is read off the lowered HIR: the Map/Set index fast path emits
//! `MapEntryKeyAt` / `MapEntryValueAt` / `SetValueAt` reads, the generic
//! protocol path emits a `GetIterator`, and the materialising fallbacks emit
//! `MapEntries` / `SetValues`.

#![cfg(test)]

use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "collection_view.ts", &mut cache)
                    .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "collection_view.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// Which lowering a `for-of` over a collection view received.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// Delete-safe index walk over the flat entries buffer — allocation-free.
    IndexFastPath,
    /// `MapEntries` / `SetValues` materialisation — a snapshot, one Array.
    Materialised,
    /// Generic iterator protocol — a `{ value, done }` object per element.
    IteratorProtocol,
}

fn route_of(src: &str) -> Route {
    let hir = format!("{:?}", lower(src));
    let indexed = hir.contains("MapEntryKeyAt")
        || hir.contains("MapEntryValueAt")
        || hir.contains("SetValueAt");
    let materialised = hir.contains("MapEntries") || hir.contains("SetValues");
    // `GetIterator` is what the lazy protocol path installs; the fast paths
    // never emit one.
    if hir.contains("GetIterator") {
        return Route::IteratorProtocol;
    }
    if indexed {
        Route::IndexFastPath
    } else if materialised {
        Route::Materialised
    } else {
        panic!("no recognizable for-of route in lowered HIR for:\n{src}");
    }
}

const MAP_DECL: &str = "const m = new Map<string, number>();\nm.set(\"a\", 1);\n";
const SET_DECL: &str = "const s = new Set<number>();\ns.add(1);\n";

fn map_src(loop_src: &str) -> String {
    format!("{MAP_DECL}{loop_src}")
}

fn set_src(loop_src: &str) -> String {
    format!("{SET_DECL}{loop_src}")
}

/// The shapes the rewrite exists for. Each must reach the index fast path —
/// the same route `for (const [k, v] of m)` already had.
#[test]
fn map_and_set_views_reach_the_index_fast_path() {
    for loop_src in [
        "for (const v of m.values()) { console.log(v); }",
        "for (const k of m.keys()) { console.log(k); }",
        "for (const [k, v] of m.entries()) { console.log(k, v); }",
        "for (const [k] of m.entries()) { console.log(k); }",
        "for (const [, v] of m.entries()) { console.log(v); }",
        "for (let v of m.values()) { console.log(v); }",
        // Single-ident heads bind one fresh `[k, v]` pair per step. Both
        // spellings are the same iteration; before this they were a
        // `MapEntries` SNAPSHOT and did not observe the body's own mutations.
        "for (const e of m) { console.log(e); }",
        "for (const e of m.entries()) { console.log(e); }",
        // the pre-existing destructured direct form, as the control
        "for (const [k, v] of m) { console.log(k, v); }",
    ] {
        assert_eq!(
            route_of(&map_src(loop_src)),
            Route::IndexFastPath,
            "map loop should take the index fast path: {loop_src}"
        );
    }
    for loop_src in [
        "for (const x of s.values()) { console.log(x); }",
        "for (const x of s.keys()) { console.log(x); }",
        "for (const x of s) { console.log(x); }",
    ] {
        assert_eq!(
            route_of(&set_src(loop_src)),
            Route::IndexFastPath,
            "set loop should take the index fast path: {loop_src}"
        );
    }
}

/// Shapes the rewrite must decline. Each is either not equivalent to the
/// direct form, or would land on a *materialising* fallback — trading the
/// iterator's live view for a snapshot, so a body that mutates the collection
/// would stop seeing its own writes.
#[test]
fn shapes_the_rewrite_must_decline_keep_the_lazy_iterator() {
    // `s.entries()` yields `[v, v]` pairs, which `for (… of s)` does not.
    assert_eq!(
        route_of(&set_src("for (const e of s.entries()) { console.log(e); }")),
        Route::IteratorProtocol,
        "Set entries() is not the direct Set iteration"
    );
    // A destructuring head over `values()` destructures the VALUE.
    assert_eq!(
        route_of(&map_src(
            "const mm = new Map<string, number[]>();\n\
             for (const [a, b] of mm.values()) { console.log(a, b); }"
        )),
        Route::IteratorProtocol,
        "values() with a destructuring head destructures the value"
    );
    // Nested patterns are not a shape the index fast path accepts.
    assert_eq!(
        route_of(&map_src(
            "for (const [[a], b] of m.entries()) { console.log(a, b); }"
        )),
        Route::IteratorProtocol,
        "nested destructuring is not an index-fast-path head"
    );
}

/// The receiver's static type is the gate. Without a `Map` / `Set` proof the
/// rewritten head means something else entirely, so an unproven receiver must
/// be left on whatever route it had.
#[test]
fn an_unproven_receiver_is_never_rewritten() {
    // A plain object with a `values()` method — rewriting would iterate the
    // object, not the method's result.
    let src = "const o = { values() { return [1, 2][Symbol.iterator](); } };\n\
               for (const v of o.values()) { console.log(v); }";
    assert_ne!(
        route_of(src),
        Route::IndexFastPath,
        "an object with a values() method is not a Map"
    );
    // A `Map` subclass types as `Type::Named`, never `Type::Generic`, so an
    // overriding `values()` is never bypassed.
    let src = "class MyMap extends Map<string, number> {}\n\
               const mm = new MyMap();\n\
               for (const v of mm.values()) { console.log(v); }";
    assert_ne!(
        route_of(src),
        Route::IndexFastPath,
        "a Map subclass may override values()"
    );
}

/// `for await` is left alone: the fast path emits no `Await` around the
/// element read, so rewriting onto it would drop the per-iteration await.
///
/// Asserted on the rewrite's own footprint rather than through `route_of` —
/// the async desugar replaces the loop wholesale, so none of the three route
/// markers survives it either way.
#[test]
fn for_await_is_never_rewritten() {
    let hir = format!(
        "{:?}",
        lower(
            "async function f(m: Map<string, number>) {\n\
             for await (const v of m.values()) { console.log(v); }\n\
             }\nf(new Map<string, number>());"
        )
    );
    assert!(
        !hir.contains("MapEntryValueAt"),
        "for await must not be routed onto the index fast path"
    );
}

/// Sabotage check: the route probe can distinguish the paths at all. If both
/// arms reported the same route the tests above would be vacuous.
#[test]
fn the_route_probe_actually_discriminates() {
    let fast = route_of(&map_src("for (const v of m.values()) { console.log(v); }"));
    let slow = route_of(&map_src(
        "for (const [[a], b] of m.entries()) { console.log(a, b); }",
    ));
    assert_eq!(fast, Route::IndexFastPath);
    assert_eq!(slow, Route::IteratorProtocol);
    assert_ne!(
        fast, slow,
        "route probe cannot tell the two lowerings apart"
    );
}
