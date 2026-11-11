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
        (globalThis as any).hostTake(function (arr: any, k: any) {{ arr[k] = 1; }});
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
        (globalThis as any).hostTake(globalThis);
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
        (globalThis as any).hostTake(console);
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
        console.warn = function (a: any) { a.size = "x"; (globalThis as any).hostKeep(a); };
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
        class Leak extends Svc { toString() { (globalThis as any).hostKeep(this); return "L"; } }
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
        class Leak extends Svc { [inspect.custom]() { (globalThis as any).hostKeep(this); return "L"; } }
        const s = new Leak();
        console.warn(s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}
