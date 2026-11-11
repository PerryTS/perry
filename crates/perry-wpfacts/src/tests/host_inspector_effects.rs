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

/// R2 (main: the HIR fold `new inspector.Session()` is a receiver-less
/// `NativeMethodCall`, whose result read ⊤ "result of native module
/// inspector.Session", the first ⊤ of tssrc's solve): the fold is `new` of
/// the realm constructor `node:inspector.Session`, whose declaration makes a
/// session object (R3), over a module alias and over a named import alike.
#[test]
fn session_fold_is_new_of_the_realm_constructor() {
    let alias = facts(&format!(
        r#"{PRELUDE}
        function enable() {{
            const inspector: typeof import("inspector") = require("inspector");
            const session = new inspector.Session();
            return h(session);
        }}
        console.log(enable());
        export const keep = 1;
    "#
    ));
    let named = facts(
        r#"
        import { Session } from "inspector";
        function h(v: any) { return v.q; }
        function enable() {
            const session = new Session();
            return h(session);
        }
        console.log(enable());
        export const keep = 1;
    "#,
    );
    for pf in [&alias, &named] {
        let q = site(pf, "q");
        assert!(
            !q.top
                && !q.receivers.is_empty()
                && q.receivers
                    .iter()
                    .all(|r| r.contains("node:inspector.Session.prototype")),
            "{:?}\n{}",
            q.receivers,
            pf.dump_text()
        );
    }
}

/// R3 (main: `new` of a builtin function value read ⊤ "`new` of an opaque
/// builtin function value"): `new` of the realm constructor's value, read off
/// the module object, runs its declaration too.
#[test]
fn new_of_declared_realm_constructor_value_runs_its_declaration() {
    let pf = facts(&format!(
        r#"{PRELUDE}
        function enable() {{
            const inspector: typeof import("inspector") = require("inspector");
            const S: any = inspector.Session;
            const session = new S(1, {{ a: 2 }});
            return h(session);
        }}
        console.log(enable());
        export const keep = 1;
    "#
    ));
    let q = site(&pf, "q");
    assert!(
        !q.top
            && !q.receivers.is_empty()
            && q.receivers
                .iter()
                .all(|r| r.contains("node:inspector.Session.prototype")),
        "{:?}\n{}",
        q.receivers,
        pf.dump_text()
    );
}

/// K4 (control): the fold calls the constructor's runtime function whatever
/// the program stored under `Session` (codegen's native row), so a program
/// class stored there is not what the fold makes; the class's own `new`
/// still makes its instance.
#[test]
fn session_fold_ignores_a_program_store_under_the_name() {
    let pf = facts(&format!(
        r#"{PRELUDE}
        class X {{ q = 1; }}
        function enable() {{
            const inspector: typeof import("inspector") = require("inspector");
            (inspector as any).Session = X;
            const session = new inspector.Session();
            const S: any = inspector.Session;
            const other = new S();
            return [h(session), other.q];
        }}
        console.log(enable());
        export const keep = 1;
    "#
    ));
    let qs: Vec<_> = pf.sites.iter().filter(|s| s.key == "q").collect();
    assert!(
        qs.iter().any(|q| !q.top
            && !q.receivers.is_empty()
            && q.receivers
                .iter()
                .all(|r| r.contains("node:inspector.Session.prototype"))),
        "{}",
        pf.dump_text()
    );
    assert!(
        qs.iter()
            .any(|q| q.top || q.receivers.iter().any(|r| r.ends_with(":X"))),
        "{}",
        pf.dump_text()
    );
}

/// K5 (control): `new` of a realm constructor whose declaration says nothing
/// of `new` stays the unknown constructor's result.
#[test]
fn new_of_undeclared_realm_constructor_stays_top() {
    let pf = facts(&format!(
        r#"{PRELUDE}
        function make() {{
            const events: any = require("events");
            const E: any = events.EventEmitter;
            return h(new E());
        }}
        console.log(make());
        export const keep = 1;
    "#
    ));
    assert!(site(&pf, "q").top, "{}", pf.dump_text());
}

/// `session.connect()` (Node inspector.Session: connects the session to the
/// inspector back-end; no arguments, returns undefined) hands nothing of the
/// program to other code: the session it is called on stays known.
#[test]
fn session_connect_keeps_the_session() {
    let pf = facts(&format!(
        r#"{PRELUDE}
        function enable() {{
            const inspector: typeof import("inspector") = require("inspector");
            const session = new inspector.Session();
            session.connect();
            return h(session);
        }}
        console.log(enable());
        export const keep = 1;
    "#
    ));
    let q = site(&pf, "q");
    assert!(
        !q.top
            && !q.receivers.is_empty()
            && q.receivers
                .iter()
                .all(|r| r.contains("node:inspector.Session.prototype")),
        "{q:?}"
    );
}
