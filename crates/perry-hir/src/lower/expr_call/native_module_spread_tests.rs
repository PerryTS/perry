//! Verdict tests for the #7720 spread bail in `lower_call`.
//!
//! These assert **which lowering a call got**, not what it computes. A
//! behaviour-only test would be a weak gate here in both directions:
//!
//!   * the broken lowering `path.join(...parts)` → `PathNormalize(<array>)`
//!     throws `ERR_INVALID_ARG_TYPE`, and so does the CORRECT lowering when the
//!     array holds a non-string — which is why
//!     `node-suite/path/join/type-errors-extra.ts` has spread-called
//!     `path.join` since long before the bug was fixed and stayed green
//!     throughout (CLAUDE.md's fourth way a gate can be unable to fail: the
//!     gate ran, its subject never did);
//!   * a regression that stopped applying the fast path *everywhere* — not
//!     just for spread calls — would also produce correct output, since the
//!     generic dispatch is a correct fallback. `join_without_spread_*` is the
//!     other half of the ratchet: it fails if the fast path stops firing.
//!
//! Byte-for-byte behaviour against node lives in
//! `test-parity/node-suite/path/{join,resolve}/spread.ts` and
//! `test-parity/node-suite/util/format/spread.ts`.

#![cfg(test)]

use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(
                &src,
                "native_module_spread.ts",
                &mut cache,
            )
            .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "native_module_spread.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn hir(src: &str) -> String {
    format!("{:?}", lower(src))
}

/// The generic tail's shape: a `CallSpread` whose callee is the namespace
/// member, which codegen dispatches through the variadic runtime by-name path.
fn declined_fast_path(src: &str) -> bool {
    hir(src).contains("CallSpread")
}

// ── the reported repro (#7720) and its sibling call forms ──────────────────

#[test]
fn join_with_spread_declines_the_static_fast_path() {
    let h = hir(r#"
        import path from 'node:path';
        const parts = ['/tmp/x', 'project.json'];
        console.log(path.join(...parts));
    "#);
    assert!(h.contains("CallSpread"), "expected CallSpread, got: {h}");
    // The bug: the spread operand folded into the one-argument arm.
    assert!(
        !h.contains("PathNormalize"),
        "still folded positionally: {h}"
    );
}

#[test]
fn join_without_spread_keeps_the_static_fast_path() {
    // The other half of the ratchet — the bail must be spread-only.
    let h = hir(r#"
        import path from 'node:path';
        console.log(path.join('/tmp/x', 'project.json'));
    "#);
    assert!(h.contains("PathJoin"), "fast path stopped firing: {h}");
    assert!(
        !h.contains("CallSpread"),
        "non-spread call was diverted: {h}"
    );
}

#[test]
fn spread_bail_covers_every_path_receiver_form() {
    // Default import, namespace import, require alias, named import, named
    // sub-namespace import, and the 3-level sub-namespace member.
    for src in [
        "import path from 'node:path'; const p = ['a','b']; console.log(path.join(...p));",
        "import * as path from 'node:path'; const p = ['a','b']; console.log(path.join(...p));",
        "const path = require('node:path'); const p = ['a','b']; console.log(path.join(...p));",
        "import { join } from 'node:path'; const p = ['a','b']; console.log(join(...p));",
        "import { posix } from 'node:path'; const p = ['a','b']; console.log(posix.join(...p));",
        "import path from 'node:path'; const p = ['a','b']; console.log(path.win32.join(...p));",
    ] {
        assert!(declined_fast_path(src), "still folded positionally: {src}");
    }
}

#[test]
fn spread_bail_is_not_path_specific() {
    // `util.format` inspected the array instead of formatting it; `fs.existsSync`
    // tested an array for existence. Same positional fold, same fix.
    for src in [
        "import util from 'node:util'; const a = ['%s', 'x']; console.log(util.format(...a));",
        "import fs from 'node:fs'; const a = ['/tmp']; console.log(fs.existsSync(...a));",
        "import os from 'node:os'; const a: string[] = []; console.log(os.homedir(...a));",
    ] {
        assert!(declined_fast_path(src), "still folded positionally: {src}");
    }
}

// ── what the bail deliberately does NOT claim ──────────────────────────────

#[test]
fn class_statics_keep_their_lowering() {
    // `Buffer` is a CLASS export of node:buffer, not a namespace. Its statics
    // are a different lowering family whose by-name runtime dispatch does not
    // cover the same surface, so they stay where they are (see
    // `native_module::is_submodule_export`).
    let h = hir(r#"
        import { Buffer } from 'node:buffer';
        const list = [Buffer.from('a'), Buffer.from('b')];
        console.log(Buffer.concat(...[list]).toString());
    "#);
    assert!(
        !h.contains("CallSpread"),
        "class static was diverted to the generic tail: {h}"
    );
}

#[test]
fn non_module_spread_intrinsics_are_untouched() {
    // `Math` / `Object` / array receivers are not node-core modules, so their
    // spread-aware fast paths keep firing.
    let h = hir("const xs = [3, 1, 2]; console.log(Math.min(...xs));");
    assert!(
        h.contains("MathMinSpread"),
        "Math.min spread lost its fast path: {h}"
    );
}
