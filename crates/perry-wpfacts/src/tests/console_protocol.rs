//! A console method formats its arguments: it calls what perry-runtime's
//! formatter calls (`Protocol::Inspect`: the `[util.inspect.custom]` hook,
//! ToPrimitive for `%s`/`%d`/`%i`/`%f`, `toJSON` for `%j`) and shows every
//! value it reaches in turn. It calls no other key: an array or instance it
//! prints does not escape (test-parity/node-suite/console/test_console_protocol_ops.ts runs the
//! same operations against the runtime).

use super::*;

fn facts_of(body: &str) -> ProgramFacts {
    facts(&format!(
        r#"
        import {{ inspect }} from "util";
        class Svc {{ size = 3; ping() {{ return 1; }} }}
        function g(a: any) {{ return a.size; }}
        function h(a: any) {{ return a.ping(); }}
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

/// three.js `error()`: the rest array goes to `console.error` by spread.
/// node: prints `3 1`.
#[test]
fn console_error_of_a_rest_array_keeps_its_elements() {
    let pf = facts_of(
        r#"
        function error(...params: any[]) {
            const message = "T." + params.shift();
            console.error(message, ...params);
        }
        const s = new Svc();
        if (Math.random() > 2) error("x:", s);
        console.log(g(s), h(s));
        "#,
    );
    assert!(proven(&pf, "size"), "{}", pf.dump_text());
    assert!(proven(&pf, "ping"), "{}", pf.dump_text());
}

/// node: prints `Svc { size: 3 } [ Svc { size: 3 } ]` then `3 1`.
#[test]
fn console_log_of_an_instance_and_an_array() {
    let pf = facts_of(
        r#"
        const s = new Svc();
        console.log(s, [s], "%s %d %j %o", s, s, s, s);
        console.log(g(s), h(s));
        "#,
    );
    assert!(proven(&pf, "size"), "{}", pf.dump_text());
    assert!(proven(&pf, "ping"), "{}", pf.dump_text());
}

/// The runtime calls a class's `[inspect.custom]` with `this` = the
/// instance (perry: `call_vtable_method`). node (`hostTake = (o) => {
/// o.size = 'x'; }`): prints `x`.
#[test]
fn a_class_inspect_hook_runs() {
    let pf = facts_of(
        r#"
        class Leak extends Svc {
            [inspect.custom]() { (globalThis as any).hostTake(this); return "L"; }
        }
        const s = new Leak();
        console.error("m", ...[s]);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// An object's symbol-keyed hook runs too, also on a value nested in what
/// is printed.
#[test]
fn a_nested_object_hook_runs() {
    let pf = facts_of(
        r#"
        const s = new Svc();
        const o: any = { [Symbol.for("nodejs.util.inspect.custom")]() {
            (globalThis as any).hostTake(s); return "O"; } };
        console.log({ inner: [o] });
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// A hook on a Map value runs.
#[test]
fn a_hook_on_a_map_value_runs() {
    let pf = facts_of(
        r#"
        class Leak extends Svc {
            [inspect.custom]() { (globalThis as any).hostTake(this); return "L"; }
        }
        const s = new Leak();
        console.log(new Map<number, any>([[1, s]]));
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// `%s` runs ToPrimitive: an own `toString` runs.
#[test]
fn format_s_runs_to_string() {
    let pf = facts_of(
        r#"
        class Leak extends Svc {
            toString() { (globalThis as any).hostTake(this); return "L"; }
        }
        const s = new Leak();
        console.log("%s", s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// `%d` runs ToPrimitive with hint number: `valueOf` runs.
#[test]
fn format_d_runs_value_of() {
    let pf = facts_of(
        r#"
        class Leak extends Svc {
            valueOf() { (globalThis as any).hostTake(this); return 1; }
        }
        const s = new Leak();
        console.warn("%d", s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// `%j` serializes: `toJSON` runs.
#[test]
fn format_j_runs_to_json() {
    let pf = facts_of(
        r#"
        class Leak extends Svc {
            toJSON() { (globalThis as any).hostTake(this); return 1; }
        }
        const s = new Leak();
        console.info("%j", s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// A getter the formatter may run (`console.dir(x, {getters: true})`).
#[test]
fn a_getter_may_run() {
    let pf = facts_of(
        r#"
        class Leak extends Svc {
            get leak() { (globalThis as any).hostTake(this); return 1; }
        }
        const s = new Leak();
        console.dir(s, { getters: true });
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}

/// The spread at the call site iterates: an overridden `Symbol.iterator`
/// runs on the array, which hands itself (and its elements) away.
#[test]
fn a_spread_runs_an_overridden_iterator() {
    let pf = facts_of(
        r#"
        function error(...params: any[]) {
            (params as any)[Symbol.iterator] = function (this: any) {
                (globalThis as any).hostTake(this); return [][Symbol.iterator]();
            };
            console.error("m", ...params);
        }
        const s = new Svc();
        error(s);
        console.log(g(s));
        "#,
    );
    assert!(!proven(&pf, "size"), "{}", pf.dump_text());
}
