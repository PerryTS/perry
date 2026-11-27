//! Lowering of the comma-callee idiom `(0, ns.m)(args)`: which lowering a call
//! got. Behaviour against node lives in the parity fixtures.

use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "sequence_callee.ts", &mut cache)
                    .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "sequence_callee.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn hir(src: &str) -> String {
    format!("{:?}", lower(src))
}

#[test]
fn comma_call_of_a_cjs_module_member_lowers_like_the_direct_call() {
    let comma = hir(r#"
        const _path = require("path");
        console.log((0, _path.join)("/a", "b", "c"));
    "#);
    let direct = hir(r#"
        const _path = require("path");
        console.log(_path.join("/a", "b", "c"));
    "#);
    assert!(
        comma.contains("PathJoin"),
        "fast path did not fire: {comma}"
    );
    assert!(direct.contains("PathJoin"));
    assert!(!comma.contains("Sequence"), "literal prefix kept: {comma}");
}

#[test]
fn comma_call_of_an_esm_namespace_member_takes_the_native_arm() {
    let h = hir(r#"
        import * as path from "node:path";
        console.log((0, path.dirname)("/a/b"));
    "#);
    assert!(h.contains("PathDirname"), "fast path did not fire: {h}");
}

#[test]
fn comma_prefix_with_effects_is_kept_in_order() {
    let h = hir(r#"
        const _path = require("path");
        let n = 0;
        console.log((n++, _path.join)("/a", "b"));
    "#);
    assert!(h.contains("PathJoin"), "fast path did not fire: {h}");
    assert!(h.contains("Sequence"), "prefix effect dropped: {h}");
}

#[test]
fn comma_call_of_a_user_object_member_keeps_the_value_call() {
    // A user object's method sees `this` = undefined: the callee stays the
    // comma expression.
    let h = hir(r#"
        const o = { m() { return this; } };
        console.log((0, o.m)());
    "#);
    assert!(h.contains("Sequence"), "comma callee was rewritten: {h}");
}

#[test]
fn comma_spread_call_keeps_the_comma_callee() {
    let h = hir(r#"
        const _path = require("path");
        const parts = ["/a", "b"];
        console.log((0, _path.join)(...parts));
    "#);
    assert!(h.contains("CallSpread"), "expected CallSpread: {h}");
    assert!(
        h.contains("Sequence"),
        "spread call lost the comma callee: {h}"
    );
}

#[test]
fn path_join_is_one_node_over_all_arguments() {
    let h = hir(r#"
        import path from "node:path";
        console.log(path.join("/a", "b", "c", "d"));
    "#);
    assert_eq!(h.matches("PathJoin").count(), 1, "chained join: {h}");
}
