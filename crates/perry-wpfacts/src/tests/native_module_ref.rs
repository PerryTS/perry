//! A node module value (`require("fs")`, `import * as fs from "fs"`) is the
//! realm's namespace object of that module (`node:fs`, `BuiltinNs::Module`),
//! not ⊤: a member read gives the declared native function value, whose
//! call runs the member's declaration when the runtime declares one.
use super::*;

/// The forms the HIR keeps as a module value (`NativeModuleRef`): a member
/// call on it is a call of the member's function value.
const FORMS: [&str; 3] = [
    "const _fs: typeof import(\"fs\") = require(\"fs\");",
    "const _fs: any = require(\"node:fs\");",
    "const _fs: any = (require(\"fs\") as any);",
];

/// `_fs.existsSync(o)` through the module value runs `existsSync`'s
/// declaration: `o` is not handed to unknown code, so `m`'s parameter holds
/// only what the program passes (main: the module value was ⊤, its member
/// an unknown callee, `o` escaped).
#[test]
fn module_value_member_runs_its_declaration() {
    for form in FORMS {
        let pf = facts(&format!(
            r#"
            class Q {{ y = 1; }}
            const o: any = {{ m(a: any) {{ return a.y; }} }};
            function sys() {{
                {form}
                return _fs.existsSync(o);
            }}
            console.log(sys());
            console.log(o.m(new Q()));
        "#
        ));
        let y = site(&pf, "y");
        assert!(
            !y.top && y.receivers.iter().all(|r| r.ends_with(":Q")),
            "{form}\n{}",
            pf.dump_text()
        );
    }
}

/// The module value is one object: `require("fs")` and `require("node:fs")`
/// read the same member value, a declared native, never ⊤.
#[test]
fn module_value_member_read_is_the_declared_native() {
    let pf = facts(
        r#"
        const a: any = require("fs");
        const b: any = require("node:fs");
        function f(v: any) { return v.q; }
        console.log(f(a.existsSync === b.existsSync), a.existsSync.name);
    "#,
    );
    assert!(!site(&pf, "name").top, "{}", pf.dump_text());
}

/// Named control: a member the runtime declares no effects for (here
/// `fs.readlinkSync`) stays the call of an opaque native: an options
/// object's getter that writes a program field is seen.
#[test]
fn module_value_undeclared_member_keeps_its_arguments_escaped() {
    let pf = facts(
        r#"
        class P { x: any; constructor(x: number) { this.x = x; } }
        const p = new P(1);
        function sys() {
            const _fs: typeof import("fs") = require("fs");
            return _fs.readlinkSync("/t", { get encoding() { p.x = "s"; return "utf8"; } } as any);
        }
        console.log(sys());
        console.log(p.x);
    "#,
    );
    let x = field(tmpl(&pf, "P"), "x");
    assert!(!x.readonly, "{}", pf.dump_text());
}

/// Named control: a key the realm does not declare on the module reads ⊤
/// (the module object is described by its declaration only); a computed
/// key may name any key.
#[test]
fn module_value_computed_key_reads_top() {
    let pf = facts(
        r#"
        const a: any = require("fs");
        const k: any = process.argv[2];
        const v: any = a[k];
        console.log(v.q);
    "#,
    );
    assert!(site(&pf, "q").top, "{}", pf.dump_text());
}

/// A function a module object holds declares own keys the model does not
/// describe: `http.ServerResponse.prototype` is the object instances
/// inherit from, where a program store reaches every response (node). The
/// read is an unknown value, never `Function.prototype`'s absent key read
/// as `undefined` (test_gap_http_proto_writes: `OM.end` was proven to be
/// read from a primitive).
#[test]
fn module_constructor_prototype_is_not_absent() {
    let pf = facts(
        r#"
        import * as http from "node:http";
        const SR: any = http.ServerResponse.prototype;
        const OM: any = http.OutgoingMessage.prototype;
        function readEnd(o: any): any { return o.end; }
        function readX(o: any): any { return o.x; }
        const r: any = new http.ServerResponse({ method: "GET" } as any);
        SR.x = 1;
        console.log(readX(r), typeof readEnd(OM), readX(SR));
    "#,
    );
    for k in ["end", "x"] {
        let s = site(&pf, k);
        assert!(s.top || !s.receivers.is_empty(), "{k}\n{}", pf.dump_text());
    }
}

/// The same for other module functions: `events.EventEmitter.prototype`
/// (a program store there reaches every emitter) and a static own key,
/// `stream.Readable.from`, a function, not `undefined`.
#[test]
fn module_function_own_keys_are_not_absent() {
    let pf = facts(
        r#"
        import * as events from "node:events";
        import * as stream from "node:stream";
        const EP: any = events.EventEmitter.prototype;
        const from: any = stream.Readable.from;
        function readQ(o: any): any { return o.q; }
        function readW(o: any): any { return o.w; }
        EP.q = 1;
        console.log(readQ(EP), readW(from));
    "#,
    );
    for k in ["q", "w"] {
        let s = site(&pf, k);
        assert!(s.top || !s.receivers.is_empty(), "{k}\n{}", pf.dump_text());
    }
}
