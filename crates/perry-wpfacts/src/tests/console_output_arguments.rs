//! A console output method keeps nothing it is given: it formats its
//! arguments (`Protocol::Inspect`, `console_args`) and returns. A store
//! into ⊤ (a parameter of a function handed to unknown code, written under
//! a computed key) does not reach the `console` object: unknown code holds
//! it only once the global object (or `console` itself) reached it, so
//! `console.warn` stays the runtime method and its argument does not
//! escape. three.js: `Color.toArray(array, offset)` called back by unknown
//! code writes `array[offset]`; `warn(...)` prints instances.

use super::*;

fn facts_of(body: &str) -> ProgramFacts {
    facts(&format!(
        r#"
        import {{ inspect }} from "util";
        class Svc {{ size = 3; ping() {{ return 1; }} }}
        function g(a: any) {{ return a.size; }}
        function h(a: any) {{ return a.ping(); }}
        // A store into ⊤ under any key, as three.js `toArray` called back.
        // (`unknown` is a value, not a method of the global object: a call
        // `globalThis.hostTake(f)` hands it the global object as `this`.)
        declare function hostValue(): any; const unknown: any = hostValue;
        unknown.take(function (arr: any, k: any) {{ arr[k] = 1; }});
        {body}
        "#
    ))
}

fn site_of<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    pf.sites
        .iter()
        .find(|s| s.key == key)
        .unwrap_or_else(|| panic!("site .{key} in\n{}", pf.dump_text()))
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    !matches!(site_of(pf, key).verdict, Verdict::No(_))
}

/// node: prints the objects, then `3 1 4 2`.
#[test]
fn console_output_methods_keep_their_arguments_past_a_top_store() {
    for method in ["log", "warn", "error", "info", "debug", "trace"] {
        let pf = facts_of(&format!(
            r#"
            const s = new Svc();
            const o = {{ size: 4, ping() {{ return 2; }}, inner: {{ s }} }};
            console.{method}("x", s, o, [s]);
            console.log(g(s), h(s), g(o), h(o));
            "#
        ));
        assert!(proven(&pf, "size"), "{method}: {}", pf.dump_text());
        assert!(proven(&pf, "ping"), "{method}: {}", pf.dump_text());
    }
}

/// Control: the global object handed to unknown code puts `console` in
/// its reach; a store into ⊤ may then replace `console.warn`.
#[test]
fn global_object_given_to_unknown_code_writes_console() {
    let pf = facts_of(
        r#"
        unknown.take2(globalThis);
        const s = new Svc();
        console.warn(s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// Control: `console` itself handed to unknown code.
#[test]
fn console_given_to_unknown_code_is_written() {
    let pf = facts_of(
        r#"
        unknown.take2(console);
        const s = new Svc();
        console.warn(s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// Control: the program replaces `console.warn`; its function runs.
#[test]
fn a_program_console_warn_runs() {
    let pf = facts_of(
        r#"
        console.warn = function (a: any) { a.size = "x"; unknown.keep(a); };
        const s = new Svc();
        console.warn(s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// Control: a `toString` the formatter calls under `%s` still runs with
/// `this` = the argument.
#[test]
fn a_to_string_under_percent_s_runs_past_a_top_store() {
    let pf = facts_of(
        r#"
        class Leak extends Svc { toString() { unknown.keep(this); return "L"; } }
        const s = new Leak();
        console.warn("%s", s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// Control: a class `[inspect.custom]` hook still runs.
#[test]
fn an_inspect_hook_runs_past_a_top_store() {
    let pf = facts_of(
        r#"
        class Leak extends Svc { [inspect.custom]() { unknown.keep(this); return "L"; } }
        const s = new Leak();
        console.warn(s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// A shown object that holds the global object (browser.ts
/// `document.defaultView = globalThis`): the formatter looks up the
/// `[util.inspect.custom]` hook of each realm object it reaches (`console`,
/// the global object) and finds none: no realm object carries the hook
/// (perry-runtime formatting.rs `inspect_custom_hook`: an own Symbol entry
/// or a class vtable slot HIR names `__perry_inspect_custom__`), so no
/// unknown function runs and the realm is not held. three.js: `log(...)`.
#[test]
fn a_realm_object_shown_runs_no_inspect_hook() {
    for method in ["log", "warn"] {
        let pf = facts_of(&format!(
            r#"
            const doc: any = {{ title: "d" }};
            doc.defaultView = globalThis;
            const s = new Svc();
            console.{method}(doc, console);
            console.warn(s);
            console.log(g(s), h(s));
            "#
        ));
        assert!(proven(&pf, "size"), "{method}: {}", pf.dump_text());
        assert!(proven(&pf, "ping"), "{method}: {}", pf.dump_text());
    }
}

/// Control: a hook the program installs on the global object runs.
#[test]
fn a_program_hook_on_the_global_object_runs() {
    let pf = facts_of(
        r#"
        const s = new Svc();
        (globalThis as any)[inspect.custom] = function () { unknown.keep(s); return "G"; };
        const doc: any = { title: "d" };
        doc.defaultView = globalThis;
        console.log(doc);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}
