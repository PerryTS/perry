//! Rule R2: the solver's formatter protocol reads a console argument only
//! as deep as perry-runtime's formatter does (builtins/formatting.rs: an
//! ordinary object past the inspect depth limit prints `[Object]` with no
//! read and no `[util.inspect.custom]` lookup; default limit 2,
//! `console.dir(x, { depth })` the options' depth; a native Error's own
//! values and a function's own values are read with no limit check, the
//! function's at depth 1). `Deep`'s hook hands `s` to unknown code: `size`
//! is proven exactly when no formatter touch runs that hook.

use super::*;

fn facts_of(body: &str) -> ProgramFacts {
    facts(&format!(
        r#"
        declare function external(x?: unknown): any;
        import {{ inspect }} from "util";
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const s = new Svc();
        class Deep {{
            [inspect.custom]() {{ external(s); return "D"; }}
        }}
        {body}
        console.log(g(s));
        "#
    ))
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let site = pf
        .sites
        .iter()
        .find(|s| s.key == key)
        .unwrap_or_else(|| panic!("site .{key} in\n{}", pf.dump_text()));
    !matches!(site.verdict, Verdict::No(_))
}

/// (a) node: prints `{ a: { b: { c: [Object] } } }`: the global object at
/// depth 3 is never read, so the `Deep` stored on it never runs its hook.
#[test]
fn console_log_reads_nothing_past_depth_two() {
    let pf = facts_of(
        r#"
        (globalThis as any).k = new Deep();
        console.log({ a: { b: { c: globalThis } } });
        "#,
    );
    assert!(proven(&pf, "size"), "{}", pf.dump_text());
}

/// (b) node: `console.dir(v, { depth: 5, customInspect: true })` prints
/// `D` at depth 5: the formatter reads that deep and runs the hook.
#[test]
fn console_dir_with_a_depth_reads_to_that_depth() {
    let pf = facts_of(
        r#"
        console.dir({ a: { b: { c: { d: { e: new Deep() } } } } }, { depth: 5, customInspect: true });
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// (c) node: an Error at depth 3 still shows its own `x` (errors.rs:107,
/// no limit check); `x` is a function, whose own `y` is shown at depth 1,
/// so `Deep` at depth 2 runs its hook.
#[test]
fn an_error_shows_its_own_values_past_the_limit() {
    let pf = facts_of(
        r#"
        const f: any = function () {};
        f.y = new Deep();
        const err: any = new Error("e");
        err.x = f;
        console.log({ a: { b: { c: err } } });
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// (d) node: a function at depth 3 shows its own values at depth 1
/// (formatting.rs:338): `{ y: D }` is read and `Deep` runs its hook.
#[test]
fn a_function_shows_its_own_values_at_depth_one() {
    let pf = facts_of(
        r#"
        const f: any = function () {};
        f.x = { y: new Deep() };
        console.log({ a: { b: { c: f } } });
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}
