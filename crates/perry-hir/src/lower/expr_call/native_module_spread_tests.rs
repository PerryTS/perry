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
fn imported_buffer_statics_with_spread_reach_the_generic_tail() {
    // `Buffer` is a CLASS export of node:buffer, not a namespace, so it is not
    // covered by `is_node_builtin_module_call`; its own lowering arms decline a
    // spread call instead (#11838). Before, `Buffer.concat(...[list])` read the
    // whole array as the one argument and `Buffer.from(...["hi", "utf8"])`
    // returned undefined.
    let h = hir(r#"
        import { Buffer } from 'node:buffer';
        const list = [Buffer.from('a'), Buffer.from('b')];
        console.log(Buffer.concat(...[list]).toString());
        console.log(Buffer.from(...(["hi", "utf8"] as [string, BufferEncoding])).toString("hex"));
    "#);
    assert!(
        h.contains("CallSpread"),
        "class static spread call was not diverted to the generic tail: {h}"
    );
    assert!(
        h.matches("CallSpread").count() >= 2,
        "both spread statics must take the generic tail: {h}"
    );
    assert!(
        !h.contains("BufferConcat") && !h.contains("module: \"buffer\""),
        "a spread Buffer static still folded positionally: {h}"
    );
}

#[test]
fn global_buffer_and_uint8array_statics_with_spread_decline() {
    for src in [
        "const a: any[] = [[]]; console.log(Buffer.concat(...a));",
        "const a: any[] = [3]; console.log(Buffer.alloc(...a));",
        "const a: any[] = [1, 2]; console.log(Uint8Array.of(...a));",
    ] {
        let h = hir(src);
        assert!(
            h.contains("CallSpread"),
            "expected CallSpread for `{src}`: {h}"
        );
        assert!(
            !h.contains("BufferConcat")
                && !h.contains("BufferAlloc")
                && !h.contains("Uint8ArrayFrom"),
            "`{src}` still folded positionally: {h}"
        );
    }
}

#[test]
fn buffer_statics_without_spread_keep_their_fast_paths() {
    let h = hir(r#"
        import { Buffer } from 'node:buffer';
        const list = [Buffer.from('a'), Buffer.from('b')];
        console.log(Buffer.concat(list).toString());
    "#);
    assert!(
        h.contains("BufferConcat"),
        "Buffer.concat lost its fast path: {h}"
    );
    assert!(!h.contains("CallSpread"), "{h}");
}

#[test]
fn sub_namespace_allowlist_is_the_runtime_bucket_set() {
    // NOT derived from `NODE_BUILTIN_MODULES`: `fs/promises` and `dns/promises`
    // are real node-core module names, but the runtime by-name dispatcher has
    // no `fs.promises` / `dns.promises` BUCKET. Re-deriving this list from the
    // module names is the exact mistake that shipped, so pin it.
    for (module, sub) in [
        ("path", "posix"),
        ("path", "win32"),
        ("util", "types"),
        ("crypto", "subtle"),
        ("punycode", "ucs2"),
    ] {
        assert!(
            super::native_module::sub_namespace_has_dispatch_bucket(module, sub),
            "{module}.{sub} should be diverted"
        );
    }
    for (module, sub) in [("fs", "promises"), ("dns", "promises"), ("stream", "web")] {
        assert!(
            !super::native_module::sub_namespace_has_dispatch_bucket(module, sub),
            "{module}.{sub} has no dispatch bucket and must keep its lowering"
        );
    }
}

#[test]
fn bucketless_sub_namespaces_keep_their_lowering() {
    // `dns.promises.lookup(...)` had a fast path (it threw
    // `ERR_INVALID_ARG_TYPE`); diverting it produced a silent `undefined`.
    // `import { promises } from 'node:fs'` registers under the `fs/promises`
    // slash tag, and diverting it turned a rejected promise into a synchronous
    // `TypeError: value is not a function`. Both must stay put.
    //
    // (`fs.promises.readFile(...args)` is deliberately absent: it already
    // reached the generic tail before this change, so a `CallSpread` there is
    // pre-existing and asserting on it would test nothing.)
    for src in [
        "import dns from 'node:dns'; const a = ['localhost']; dns.promises.lookup(...a);",
        "import { promises } from 'node:fs'; const a = ['/x','utf8']; promises.readFile(...a);",
    ] {
        let h = hir(src);
        assert!(
            !h.contains("CallSpread"),
            "bucket-less sub-namespace was diverted: {src}"
        );
    }
}

#[test]
fn bucket_backed_sub_namespaces_are_diverted() {
    // The dotted tags `nm_module_index` really has a bucket for.
    for src in [
        "import path from 'node:path'; const a = ['/x','y']; console.log(path.posix.join(...a));",
        "import path from 'node:path'; const a = ['/x','y']; console.log(path.win32.join(...a));",
        "import util from 'node:util'; const a = [new Date()]; console.log(util.types.isDate(...a));",
    ] {
        assert!(declined_fast_path(src), "still folded positionally: {src}");
    }
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

#[test]
fn mixed_fixed_and_spread_math_calls_keep_the_spread_marker() {
    for src in [
        "const xs = [3, 1, 2]; console.log(Math.min(-1, ...xs));",
        "const xs = [3, 1, 2]; console.log(Math.max(-1, ...xs));",
        "const xs = [3, 1, 2]; console.log(Math['max'](-1, ...xs));",
    ] {
        let h = hir(src);
        assert!(
            h.contains("CallSpread"),
            "a mixed Math call must retain its fixed/spread argument boundary: {h}"
        );
        assert!(
            !h.contains("MathMin([") && !h.contains("MathMax(["),
            "the spread tail must not be coerced as one scalar Math argument: {h}"
        );
    }
}

// ── JS built-in statics and global functions with a spread argument ────────
//
// prettier's `getSupportInfo` merges every plugin's option table with
// `Object.assign({}, ...plugins.map(({ options }) => options), core)`. The
// `Object.assign` fast path took the spread operand as ONE source, so the
// merged table held the array's indices ("0", "1") instead of the options,
// and `prettier.format` threw `Unexpected type undefined`. The same
// positional folding hit `JSON.stringify(...)`, `Number.isInteger(...)`,
// `Array.of(...)`, `parseInt(...)` and the rest of those arms.
// Behaviour against node: `test-files/test_gap_builtin_static_spread_args.ts`.

#[test]
fn object_assign_with_spread_declines_the_static_fast_path() {
    let h = hir(r#"
        const plugins: any[] = [{ name: "a" }, { options: { x: 1 } }];
        const merged = Object.assign({}, ...plugins.map(({ options }) => options), { y: 2 });
        console.log(merged);
    "#);
    assert!(h.contains("CallSpread"), "expected CallSpread, got: {h}");
    assert!(
        !h.contains("ObjectAssign"),
        "Object.assign still folded the spread operand positionally: {h}"
    );
}

#[test]
fn object_assign_without_spread_keeps_the_static_fast_path() {
    let h = hir("const t: any = {}; console.log(Object.assign(t, { x: 1 }, { y: 2 }));");
    assert!(
        h.contains("ObjectAssign"),
        "Object.assign lost its fast path: {h}"
    );
}

#[test]
fn builtin_statics_with_spread_decline_their_fast_paths() {
    for (src, folded) in [
        (
            "const a: any[] = [{ k: 1 }]; console.log(Object.keys(...a));",
            "ObjectKeys",
        ),
        (
            "function f(a: any[]) { return Array.of(...a); } console.log(f([1]));",
            "Array([LocalGet",
        ),
        (
            "function f(a: any[]) { return Math.hypot(...a); } console.log(f([3, 4]));",
            "MathHypot",
        ),
        (
            "const a: any[] = [{ k: 1 }]; console.log(Reflect.ownKeys(...a));",
            "ReflectOwnKeys",
        ),
        (
            "const a: any[] = [{ k: 1 }]; console.log(JSON.stringify(...a));",
            "JsonStringify",
        ),
        (
            "const a: any[] = [2]; console.log(Number.isInteger(...a));",
            "NumberIsInteger",
        ),
        (
            "const a: any[] = [\"ff\", 16]; console.log(parseInt(...a));",
            "ParseInt",
        ),
    ] {
        let h = hir(src);
        assert!(
            h.contains("CallSpread"),
            "expected CallSpread for `{src}`, got: {h}"
        );
        assert!(
            !h.contains(folded),
            "`{src}` still folded into {folded}: {h}"
        );
    }
}

#[test]
fn builtin_statics_without_spread_keep_their_fast_paths() {
    for (src, folded) in [
        (
            "const o: any = { k: 1 }; console.log(Object.keys(o));",
            "ObjectKeys",
        ),
        (
            "const n: any = 2; console.log(Number.isInteger(n));",
            "NumberIsInteger",
        ),
        (
            "const s: any = \"ff\"; console.log(parseInt(s, 16));",
            "ParseInt",
        ),
    ] {
        let h = hir(src);
        assert!(
            h.contains(folded),
            "`{src}` lost its {folded} fast path: {h}"
        );
    }
}

#[test]
fn builtin_static_spread_call_keeps_its_namespace_receiver() {
    // The collapsed static surface `PropertyGet { GlobalGet(0), "stringify" }`
    // has no receiver for the spread dispatch ("value is not a function").
    let h = hir("const a: any[] = [{ k: 1 }]; console.log(JSON.stringify(...a));");
    assert!(
        h.contains("CallSpread { callee: PropertyGet { object: PropertyGet { object: GlobalGet(0), property: \"JSON\""),
        "the spread call lost its JSON receiver: {h}"
    );
}

// ── built-in constructors, BigInt and Symbol with a spread argument (#11838) ──
//
// Every per-constructor `new` branch reads its arguments positionally, so
// `new Date(...[2020, 0, 2])` handed the Date the one array (NaN) and
// `new Set(...[[1]])` built an empty set. `BigInt(...xs)` / `Symbol(...xs)`
// coerced / described the array itself. Behaviour against node:
// `test-files/test_gap_builtin_ctor_spread_args.ts`.

#[test]
fn builtin_constructors_with_spread_construct_the_global_by_value() {
    for name in [
        "Date",
        "Map",
        "Set",
        "WeakMap",
        "WeakSet",
        "Error",
        "TypeError",
        "AggregateError",
        "Array",
        "RegExp",
        "Number",
        "String",
        "Boolean",
        "Object",
        "ArrayBuffer",
        "Uint8Array",
        "Float64Array",
        "DataView",
        "Promise",
        "Proxy",
        "WeakRef",
        "URL",
    ] {
        let h = hir(&format!(
            "function f(a: any[]) {{ return new {name}(...a); }} console.log(f([]));"
        ));
        assert!(
            h.contains(&format!(
                "NewDynamicSpread {{ callee: PropertyGet {{ object: GlobalGet(0), property: \"{name}\""
            )),
            "`new {name}(...a)` did not construct the global by value: {h}"
        );
    }
}

#[test]
fn builtin_constructors_without_spread_keep_their_lowering() {
    for (src, folded) in [
        ("console.log(new Date(2020, 0, 2));", "DateNew"),
        ("console.log(new Map([[1, 2]]));", "MapNewFromArray"),
        ("console.log(new Set([1]));", "SetNewFromArray"),
    ] {
        let h = hir(src);
        assert!(h.contains(folded), "`{src}` lost {folded}: {h}");
        assert!(!h.contains("NewDynamicSpread"), "`{src}`: {h}");
    }
}

#[test]
fn shadowed_builtin_constructor_names_do_not_reach_the_global() {
    // A parameter, a local and a user class of the same name are not the
    // global: the spread must construct THAT binding.
    for src in [
        "function f(Date: any, a: any[]) { return new Date(...a); } console.log(f(Array, []));",
        "const Map: any = Array; const a: any[] = []; console.log(new Map(...a));",
        "class Set { constructor(..._r: any[]) {} } const a: any[] = []; console.log(new Set(...a));",
        "function Date(this: any, ..._r: any[]) {} const a: any[] = []; console.log(new (Date as any)(...a));",
        "const a: any[] = []; console.log(new Map(...a)); class Map { constructor(..._r: any[]) {} }",
    ] {
        let h = hir(src);
        assert!(
            !h.contains("GlobalGet(0), property: \"Date\"")
                && !h.contains("GlobalGet(0), property: \"Map\"")
                && !h.contains("GlobalGet(0), property: \"Set\""),
            "`{src}` constructed the global instead of the binding: {h}"
        );
    }
}

#[test]
fn imported_builtin_constructor_names_do_not_reach_the_global() {
    // `URL` imported from node:url is the import's binding, not the global:
    // the spread arm must not rebuild it from `globalThis.URL`.
    let h = hir(
        "import { URL } from \"node:url\"; const a: any[] = [\"/p\"]; console.log(new URL(...a));",
    );
    assert!(
        !h.contains("GlobalGet(0), property: \"URL\""),
        "an imported constructor was rebuilt from the global: {h}"
    );
}

#[test]
fn bigint_and_symbol_with_spread_read_the_expanded_first_argument() {
    for (src, call) in [
        (
            "const a: any[] = [42]; console.log(BigInt(...a));",
            "BigIntCoerce(IndexGet { object: ArraySpread(",
        ),
        (
            "const a: any[] = [\"d\"]; console.log(Symbol(...a));",
            "SymbolNew(Some(IndexGet { object: ArraySpread(",
        ),
    ] {
        let h = hir(src);
        assert!(
            h.contains(call),
            "`{src}` did not take the first expanded element: {h}"
        );
        assert!(h.contains("Spread("), "the spread marker was lost: {h}");
    }
}

#[test]
fn bigint_and_symbol_without_spread_keep_their_lowering() {
    let h = hir("const a: any = 42; console.log(BigInt(a), Symbol(\"d\"));");
    assert!(h.contains("BigIntCoerce(LocalGet"), "{h}");
    assert!(h.contains("SymbolNew(Some(String"), "{h}");
    assert!(!h.contains("ArraySpread"), "{h}");
}

#[test]
fn more_builtin_statics_with_spread_decline_their_fast_paths() {
    for (src, folded) in [
        (
            "const a: any[] = [2020, 0, 2]; console.log(Date.UTC(...a));",
            "DateUtc",
        ),
        (
            "const a: any[] = [\"2020-01-02\"]; console.log(Date.parse(...a));",
            "DateParse",
        ),
        (
            "const a: any[] = [new Uint8Array(1)]; console.log(ArrayBuffer.isView(...a));",
            "isArrayBufferView",
        ),
        (
            "const a: any[] = [\"/p\", \"http://h.test\"]; console.log(URL.canParse(...a));",
            "UrlCanParse",
        ),
        (
            "const a: any[] = [1, 2]; console.log(Float64Array.of(...a));",
            "TypedArrayNew",
        ),
        (
            "const a: any[] = [[1, 2]]; console.log(Int32Array.from(...a));",
            "TypedArrayNew",
        ),
        (
            "const a: any[] = [8, 257n]; console.log(BigInt.asUintN(...a));",
            "module: \"bigint\"",
        ),
    ] {
        let h = hir(src);
        assert!(
            h.contains("CallSpread"),
            "expected CallSpread for `{src}`: {h}"
        );
        assert!(
            !h.contains(folded),
            "`{src}` still folded into {folded}: {h}"
        );
    }
}

#[test]
fn more_builtin_statics_without_spread_keep_their_fast_paths() {
    for (src, folded) in [
        ("console.log(Date.UTC(2020, 0, 2));", "DateUtc"),
        (
            "console.log(ArrayBuffer.isView(new Uint8Array(1)));",
            "isArrayBufferView",
        ),
        ("console.log(Float64Array.of(1, 2));", "TypedArrayNew"),
        (
            "console.log(BigInt.asUintN(8, 257n));",
            "module: \"bigint\"",
        ),
    ] {
        let h = hir(src);
        assert!(
            h.contains(folded),
            "`{src}` lost its {folded} fast path: {h}"
        );
    }
}

/// #11896: `ns.Buffer.compare(a, b)` through a namespace import built a
/// `NativeMethodCall` with class `Buffer` that no codegen table dispatches, so
/// every static on it (`from`, `compare`, `concat`, ...) evaluated to
/// `undefined`. The call now reaches the generic path on the `Buffer` value.
#[test]
fn namespace_import_buffer_statics_are_not_a_native_class_call() {
    for src in [
        "import * as ns from \"node:buffer\"; console.log(ns.Buffer.compare(ns.Buffer.from(\"a\"), ns.Buffer.from(\"b\")));",
        "import * as ns from \"buffer\"; console.log(ns.Buffer.concat([]));",
    ] {
        let h = hir(src);
        assert!(
            !h.contains("class_name: Some(\"Buffer\")"),
            "`{src}` lowered to a receiver-less class NativeMethodCall: {h}"
        );
    }
}
