//! `then`, `catch` and `finally` construct a program species with
//! NewPromiseCapability (ECMA-262 27.2.1.5): `new C(executor)`, where the
//! executor is a runtime function whose closure is the capability. Called
//! with `(resolve, reject)` it keeps them; a second call throws a TypeError.
//! The reaction jobs then call what it kept: resolve with the value the
//! derived promise is resolved with, reject with the rejection reason. The
//! executor is that known function (`builtins::capability_executor`), not
//! an unknown value: the functions it is given are called with those values
//! and do not escape.

use super::*;

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

fn program(body: &str) -> String {
    format!(
        r#"
        class Sym {{ sv: number; constructor(sv: number) {{ this.sv = sv; }} }}
        function g(o: any) {{ return o.sv; }}
        const s = new Sym(1);
        {body}
        "#
    )
}

/// The verdict of the one `sv` read (in `g`).
fn sv(body: &str) -> Verdict {
    site(&facts(&program(body)), "sv").verdict.clone()
}

/// The fixture. node: prints `1`. `then` constructs `C` with the
/// capability's executor; `C` calls it with its own resolve and reject;
/// the reaction resolves the capability with the handler's result `s`.
#[test]
fn the_species_resolve_gets_the_handler_result() {
    let v = sv(r#"
        class C { constructor(ex: any) { ex((x: any) => { console.log(g(x)); }, (e: any) => {}); } }
        (C as any)[Symbol.species] = C;
        const p: any = Promise.resolve(0);
        p.constructor = C;
        p.then(() => s);
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`. `catch` of a fulfilled promise passes its value
/// through: the species' resolve gets `s`.
#[test]
fn the_species_resolve_gets_the_passed_through_value() {
    let v = sv(r#"
        class C { constructor(ex: any) { ex((x: any) => { console.log(g(x)); }, (e: any) => {}); } }
        (C as any)[Symbol.species] = C;
        const p: any = Promise.resolve(s);
        p.constructor = C;
        p.catch(() => s);
    "#);
    assert!(proven(&v), "{v:?}");
}

/// Control (stores the executor). node: prints `1`. The constructor keeps
/// the executor in a field and calls it from there. The call through the
/// field reaches a function only the runtime has: that call site stays
/// unproven (codegen has no body to call).
#[test]
fn a_stored_executor_is_the_same_function() {
    let pf = facts(&program(
        r#"
        class C {
            ex: any;
            constructor(ex: any) { this.ex = ex; this.ex((x: any) => { console.log(g(x)); }, () => {}); }
        }
        (C as any)[Symbol.species] = C;
        const p: any = Promise.resolve(0);
        p.constructor = C;
        p.then(() => s);
    "#,
    ));
    let v = &site(&pf, "sv").verdict;
    assert!(proven(v), "{v:?}");
    let call = pf
        .sites
        .iter()
        .find(|s| s.key == "ex" && s.is_call)
        .unwrap_or_else(|| panic!("call site .ex in\n{}", pf.dump_text()));
    assert!(!proven(&call.verdict), "{:?}", call.verdict);
}

/// Control (calls it twice). node: prints `1`, then `undefined`: the second
/// call throws a TypeError (the capability already holds a resolve), which
/// the constructor catches. The catch holds the runtime's error object, so
/// `caught` is not only `s`.
#[test]
fn a_second_call_throws_into_the_catch() {
    let v = sv(r#"
        let caught: any = s;
        class C {
            constructor(ex: any) {
                ex(() => {}, () => {});
                try { ex(() => {}, () => {}); } catch (e) { caught = e; }
            }
        }
        (C as any)[Symbol.species] = C;
        const p: any = Promise.resolve(0);
        p.constructor = C;
        p.then(() => s);
        console.log(g(caught));
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// Control (never calls it). node: prints `undefined`: the capability has
/// no resolve, so `then` throws a TypeError after the construct.
#[test]
fn an_executor_never_called_makes_then_throw() {
    let v = sv(r#"
        let caught: any = s;
        class C { constructor(ex: any) {} }
        (C as any)[Symbol.species] = C;
        const p: any = Promise.resolve(0);
        p.constructor = C;
        try { p.then(() => s); } catch (e) { caught = e; }
        console.log(g(caught));
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// Control: an executor handed to unknown code. Unknown code may call it
/// with its own functions, which the reaction then gives `s`: `s` escapes.
#[test]
fn an_escaped_executor_hands_out_the_settled_value() {
    let v = sv(r#"
        class C { constructor(ex: any) { (globalThis as any).hook(ex); } }
        (C as any)[Symbol.species] = C;
        const p: any = Promise.resolve(0);
        p.constructor = C;
        p.then(() => s);
        console.log(g(s));
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// Control: `finally` resolves the capability with the result of `then`
/// invoked on PromiseResolve(C, onFinally()) (27.2.5.3), the species' own
/// method: any value.
#[test]
fn a_finally_capability_is_resolved_with_any_value() {
    let v = sv(r#"
        class C { constructor(ex: any) { ex((x: any) => { console.log(g(x)); }, (e: any) => {}); } }
        (C as any)[Symbol.species] = C;
        const p: any = Promise.resolve(s);
        p.constructor = C;
        p.finally(() => {});
    "#);
    assert!(!proven(&v), "{v:?}");
}
