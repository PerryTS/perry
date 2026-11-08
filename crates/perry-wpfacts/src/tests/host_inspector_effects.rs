//! `node:inspector` (HOST slice I). `require("inspector")` is a CommonJS-style
//! default import: the HIR reads `default` off the module object. The realm
//! declares that data property's value, the node module object
//! `node:inspector.default`, so the read is that object (rule R1), and its
//! `Session` is the declared native, not ⊤.
use super::*;

const PRELUDE: &str = r#"
    declare function require(id: string): any;
    function h(v: any) { return v.q; }
"#;

/// F1 (main: `default` read ⊤ "native module ref node:inspector.default",
/// so `.Session` read from ⊤): the module object's `Session` is the realm's
/// constructor function value.
#[test]
fn cjs_default_reads_the_declared_module_object() {
    let pf = facts(&format!(
        r#"{PRELUDE}
        function enable() {{
            const inspector: typeof import("inspector") = require("inspector");
            const S: any = inspector.Session;
            return [h(S), S.name];
        }}
        console.log(enable());
        export const keep = 1;
    "#
    ));
    for key in ["Session", "name"] {
        let s = site(&pf, key);
        assert!(
            !s.top && !s.receivers.is_empty(),
            "{key}\n{}",
            pf.dump_text()
        );
    }
    let q = site(&pf, "q");
    assert!(
        !q.top
            && !q.receivers.is_empty()
            && q.receivers
                .iter()
                .all(|r| r.contains("node:inspector.Session")),
        "{:?}\n{}",
        q.receivers,
        pf.dump_text()
    );
}

/// K2 (control): a program store of `Session` onto the module object: the
/// read is the program's value (or ⊤), never the declared native alone.
#[test]
fn program_store_over_declared_member_is_read() {
    let pf = facts(&format!(
        r#"{PRELUDE}
        class X {{ z = 1; }}
        function enable() {{
            const inspector: typeof import("inspector") = require("inspector");
            (inspector as any).Session = X;
            const S: any = inspector.Session;
            return h(S);
        }}
        console.log(enable());
        export const keep = 1;
    "#
    ));
    let q = site(&pf, "q");
    assert!(
        q.top || q.receivers.iter().any(|r| r.ends_with(":X")),
        "{:?}\n{}",
        q.receivers,
        pf.dump_text()
    );
}

/// K3 (control): a computed key the realm does not declare on the module
/// object reads ⊤.
#[test]
fn computed_key_on_module_object_reads_top() {
    let pf = facts(&format!(
        r#"{PRELUDE}
        function enable(k: string) {{
            const inspector: typeof import("inspector") = require("inspector");
            const v: any = (inspector as any)["x" + k];
            return h(v);
        }}
        console.log(enable(process.argv[2]));
        export const keep = 1;
    "#
    ));
    assert!(site(&pf, "q").top, "{}", pf.dump_text());
}

/// S1 (soundness): `module._cache`, `http.globalAgent.sockets` and
/// `util.inspect.colors` are runtime-owned data objects the realm now
/// declares. A program object stored into one is reachable from the runtime
/// and other modules, so it must still escape. C is the control: the same
/// shape stored into a plain local object stays unescaped.
#[test]
fn object_stored_into_runtime_owned_data_object_escapes() {
    let pf = facts(&format!(
        r#"{PRELUDE}
        class A {{ v = 1; }}
        class B {{ v = 2; }}
        class C {{ v = 3; }}
        function put(k: string) {{
            (require("module") as any)._cache[k] = new A();
            (require("http") as any).globalAgent.sockets[k] = new B();
            const local: any = {{}};
            local[k] = new C();
            return local[k].v;
        }}
        console.log(put(process.argv[2]));
        export const keep = 1;
    "#
    ));
    for t in ["A", "B"] {
        assert!(tmpl(&pf, t).escape.is_some(), "{t}\n{}", pf.dump_text());
    }
    assert!(tmpl(&pf, "C").escape.is_none(), "{}", pf.dump_text());
}
