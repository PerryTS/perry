//! Rule R2: the solver's formatter protocol reads a console argument only
//! as deep as perry-runtime's formatter does (builtins/formatting.rs: an
//! ordinary object past the inspect depth limit prints `[Object]` with no
//! read and no `[util.inspect.custom]` lookup; default limit 2,
//! `console.dir(x, { depth })` the options' depth; a native Error's own
//! values and a function's own values are read with no limit check, the
//! function's at depth 1). `Deep`'s hook hands the `s` its receiver
//! holds to unknown code (the declared `external`, a host function; a
//! `globalThis.hostTake` that nothing stores is an absent key, whose call
//! throws and hands nothing on). The hook reaches `s` only through `this`,
//! which only a call binds (the solver generates every body, so a leak of
//! a captured `s` would show whether or not the hook runs): `size` is
//! proven exactly when no formatter touch runs the hook. The controls check
//! both directions: a touch that must run the hook makes `size` unproven,
//! and a `Deep` nobody prints leaves it proven.

use super::*;

fn facts_of(body: &str) -> ProgramFacts {
    facts(&format!(
        r#"
        import {{ inspect }} from "util";
        declare function external(x: unknown): void;
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const s = new Svc();
        class Deep {{
            h: any = s;
            [inspect.custom]() {{ external(this.h); return "D"; }}
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

/// Control: node prints `D`: the hook of a console argument runs.
#[test]
fn control_a_hook_at_depth_zero_runs() {
    let pf = facts_of("console.log(new Deep());");
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// Control: node prints `{ a: D }`: a value at depth 1 is within the
/// default limit, so its hook runs.
#[test]
fn control_a_hook_at_depth_one_runs() {
    let pf = facts_of("console.log({ a: new Deep() });");
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// Control: node prints `{ a: { b: D } }`: depth 2 is the last level read
/// under the default limit, so the hook runs.
#[test]
fn control_a_hook_at_depth_two_runs() {
    let pf = facts_of("console.log({ a: { b: new Deep() } });");
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// Control: node prints `{ a: { b: { c: [Object] } } }`: the ordinary
/// object at depth 3 is not read, so `Deep` at depth 4 runs no hook (the
/// twin of (a) without the global object).
#[test]
fn control_a_hook_at_depth_four_does_not_run() {
    let pf = facts_of("console.log({ a: { b: { c: { d: new Deep() } } } });");
    assert!(proven(&pf, "size"), "{}", pf.dump_text());
}


/// Control: a `Deep` that is never printed runs no hook.
#[test]
fn control_a_hook_nobody_prints_does_not_run() {
    let pf = facts_of("const d = new Deep();");
    assert!(proven(&pf, "size"), "{}", pf.dump_text());
}
