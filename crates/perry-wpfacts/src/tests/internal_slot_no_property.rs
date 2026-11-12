//! An internal slot (ECMA-262 6.1.7.2; [[PromiseResult]], 27.2.6) is no
//! property: no computed key writes or reads it, no spread or
//! `Object.assign` copies it, no for-in lists it. A value that is no promise
//! has no [[PromiseResult]], so `await` of it (27.2.1.3.2 step 9: a
//! non-callable `then` fulfills with the value itself) is the value, never
//! what its properties hold.
use super::*;

fn sites(pf: &ProgramFacts, key: &str, call: bool) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn proven(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::T0Mono | Verdict::T0Set))
}

fn open(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().any(|v| matches!(v, Verdict::No(_)))
}

/// A spread of a promise copies no [[PromiseResult]]: `await` of the copy
/// is the copy, so the store `r.m = 5` writes the copy, not `c`.
#[test]
fn spread_of_a_promise_copies_no_result_slot() {
    let pf = facts(
        r#"
        class C { m() { return 1; } }
        const c = new C();
        const q: any = { ...Promise.resolve(c) };
        const q2: any = Object.assign({}, Promise.resolve(c));
        async function main() {
            const r: any = await q;
            r.m = 5;
            const r2: any = await q2;
            r2.m = 6;
            console.log(c.m());
        }
        main();
        "#,
    );
    let v = sites(&pf, "m", true);
    assert!(proven(&v), "{v:?}\n{}", pf.dump_text());
}

/// A computed-key store into a JSON object writes no [[PromiseResult]]:
/// `await` of it is the object (its `then` holds no function).
#[test]
fn computed_store_writes_no_result_slot() {
    let pf = facts(
        r#"
        class C { m() { return 1; } }
        const c = new C();
        const j: any = JSON.parse(process.argv[2]);
        j[process.argv[3]] = c;
        async function main() {
            const r: any = await j;
            r.m = 5;
            console.log(c.m());
        }
        main();
        "#,
    );
    let v = sites(&pf, "m", true);
    assert!(proven(&v), "{v:?}\n{}", pf.dump_text());
}

/// Control: the settled value of a promise is still read by its name.
#[test]
fn awaited_promise_still_yields_its_value() {
    let pf = facts(
        r#"
        class C { m() { return 1; } }
        const c = new C();
        async function main() {
            const r: any = await Promise.resolve(c);
            r.m = 5;
            console.log(c.m());
        }
        main();
        "#,
    );
    let v = sites(&pf, "m", true);
    assert!(open(&v), "{v:?}\n{}", pf.dump_text());
}

/// Control: unknown code may be a thenable that passes anything it holds.
#[test]
fn awaited_unknown_value_stays_open() {
    let pf = facts(
        r#"
        class C { m() { return 1; } }
        const c = new C();
        const u: any = new Function(process.argv[2])();
        u(c);
        async function main() {
            const r: any = await u;
            r.m = 5;
            console.log(c.m());
        }
        main();
        "#,
    );
    let v = sites(&pf, "m", true);
    assert!(open(&v), "{v:?}\n{}", pf.dump_text());
}

/// Control: a key from `process.argv` may be `then`; a function stored
/// there makes the object a thenable whose resolve receives `c`.
#[test]
fn computed_then_store_of_a_function_stays_open() {
    let pf = facts(
        r#"
        class C { m() { return 1; } }
        const c = new C();
        const o: any = {};
        o[process.argv[2]] = (res: any) => res(c);
        async function main() {
            const r: any = await o;
            r.m = 5;
            console.log(c.m());
        }
        main();
        "#,
    );
    let v = sites(&pf, "m", true);
    assert!(open(&v), "{v:?}\n{}", pf.dump_text());
}
