//! Test-binary definitions for the fetch symbols `perry-runtime` CALLS but
//! this crate does not own (#8155).
//!
//! # Why this exists
//!
//! `perry-runtime` is built here with `external-fetch-symbols`, which is
//! correct for every shipped configuration: it declares
//! `js_blob_new` / `js_file_new` / `js_headers_init_from_value` /
//! `js_fetch_notify_signal_aborted` as `extern` and *calls* them
//! (`object/global_fetch.rs`, `url/abort.rs`), on the promise that someone
//! else in the final link defines them. In a real binary that someone is
//! `perry-stdlib` (`fetch_blob.rs`, `fetch/abort_bridge.rs`).
//!
//! This crate's own test binary links `perry-runtime` and nothing else, so
//! those calls have no definition and `cargo test --no-run` fails to link —
//! which is what kept the `ext-link` gate red on every PR while
//! `cargo-test`'s scope deliberately keeps `perry-ext-*` out of its fan-out
//! (#7656). The gate for the whole ext family was therefore dark.
//!
//! # Why `#[cfg(test)]` rather than a real implementation
//!
//! This crate is a `staticlib` whose objects WIN the final link ahead of
//! perry-stdlib (`prefer_well_known_before_stdlib`). Defining these four for
//! real here would therefore *replace* perry-stdlib's Blob/File/Headers
//! constructors and abort bridge in every shipped binary — a behaviour change
//! smuggled in under a CI fix, and one that would have to reimplement
//! `FETCH_ABORT_WATCHERS` and the blob registry to be equivalent. This crate
//! implements 39 fetch symbols; these four are deliberately not among them.
//!
//! `#[cfg(test)]` is compiled only into the test harness, never into
//! `libperry_ext_fetch.a`, so the shipped surface is byte-identical and no
//! duplicate symbol can reach a real link. The
//! `shipped_staticlib_does_not_define_stdlib_owned_fetch_symbols` test below
//! is the standing proof of that.
//!
//! Semantics match `perry-runtime`'s own `stdlib_stubs.rs`: warn once, return
//! `undefined`. A test that needs real Blob/File behaviour needs perry-stdlib
//! in its link, and should say so rather than silently getting a no-op.

use std::sync::atomic::{AtomicBool, Ordering};

fn warn_once(name: &str) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "perry-ext-fetch: test-only stub `{name}` called — perry-stdlib owns \
             the real implementation and is not in this test binary's link (#8155)"
        );
    }
}

/// `undefined`, NaN-boxed. Spelled out rather than imported so this module
/// depends on nothing that could drift it away from the runtime's ABI.
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

#[no_mangle]
pub extern "C" fn js_blob_new(_parts: f64, _type_value: f64) -> f64 {
    warn_once("js_blob_new");
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_file_new(
    _parts: f64,
    _name: f64,
    _type_value: f64,
    _last_modified: f64,
) -> f64 {
    warn_once("js_file_new");
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_headers_init_from_value(_handle: f64, _init: f64) -> f64 {
    warn_once("js_headers_init_from_value");
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_fetch_notify_signal_aborted(_signal_ptr: i64) {
    warn_once("js_fetch_notify_signal_aborted");
}

#[cfg(test)]
mod tests {
    /// The four symbols above must never reach the shipped archive.
    ///
    /// They are `#[cfg(test)]`, so this asserts a property of the build
    /// configuration rather than of the source: if someone later moves this
    /// module out from behind `cfg(test)` — or the crate starts exporting a
    /// real one of these — `libperry_ext_fetch.a` would begin overriding
    /// perry-stdlib's constructors at the final link, silently, because ext
    /// archives are linked first. Reading the crate's own source for a
    /// non-`cfg(test)` definition is the check that survives that move.
    #[test]
    fn shipped_staticlib_does_not_define_stdlib_owned_fetch_symbols() {
        // Read the GATE, not this file. An earlier version scanned
        // `test_link_stubs.rs` for `#[cfg(test)]` and found the annotation on
        // the inner `mod tests` below — so deleting the gate in `lib.rs`, the
        // one thing that keeps these four symbols out of the shipped archive,
        // left the assertion passing. The fact under test lives in lib.rs.
        let lib_rs = include_str!("lib.rs");
        let declaration = lib_rs
            .find("mod test_link_stubs;")
            .expect("lib.rs must declare the test_link_stubs module");
        let preceding = &lib_rs[..declaration];
        assert!(
            preceding.trim_end().ends_with("#[cfg(test)]"),
            "`mod test_link_stubs;` in lib.rs must be immediately preceded by \
             #[cfg(test)] — without it these four symbols land in \
             libperry_ext_fetch.a and override perry-stdlib's implementations \
             at the final link, because ext archives are linked first"
        );

        for symbol in [
            "js_blob_new",
            "js_file_new",
            "js_headers_init_from_value",
            "js_fetch_notify_signal_aborted",
        ] {
            let defined_outside_this_module = crate_sources_defining(symbol);
            assert!(
                defined_outside_this_module.is_empty(),
                "{symbol} is defined in the shipped surface ({defined_outside_this_module:?}); \
                 that overrides perry-stdlib's implementation at the final link, because ext \
                 archives are linked before stdlib"
            );
        }
    }

    /// Files of this crate (excluding this test-only module) that define
    /// `symbol` as a `#[no_mangle]` export.
    fn crate_sources_defining(symbol: &str) -> Vec<String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let needle = format!("fn {symbol}(");
        let mut hits = Vec::new();
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs")
                    || path.file_name().is_some_and(|n| n == "test_link_stubs.rs")
                {
                    continue;
                }
                if std::fs::read_to_string(&path)
                    .is_ok_and(|text| text.contains(&needle) && text.contains("no_mangle"))
                {
                    hits.push(path.display().to_string());
                }
            }
        }
        hits
    }
}
