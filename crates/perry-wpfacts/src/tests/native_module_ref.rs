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
