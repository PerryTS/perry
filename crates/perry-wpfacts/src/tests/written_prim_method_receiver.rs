//! `ext[k].m(a)` on a plain object literal under a computed key: the value
//! is one of the literal's fields or a member of `Object.prototype`. A store
//! into ⊤ under an unknown key may write `Object.prototype.constructor` (the
//! lock exempts it) or add a key to `String.prototype`; the key then holds
//! what the store stored (ECMA-262 10.1.9.2), not an unknown value: a read
//! yields the stored value, a call calls it (a value that is not callable
//! throws), and `a` is handed to no unknown method (three.js GLTFLoader
//! `createDracoPrimitive`).
//! Controls: a method really written onto `String.prototype` by unknown code
//! called on a string keeps `a` escaping; so does a receiver parsed from
//! the command line, a stored value made by unknown code, and a getter
//! defined through ⊤. A stored function is called with `a`; a delete
//! through ⊤ stores nothing.
use super::*;

fn sites(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites.iter().filter(|s| s.key == key && s.is_call).map(|s| s.verdict.clone()).collect()
}

fn proven(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::T0Mono | Verdict::T0Set))
}

fn program(call: &str) -> String {
    format!(
        r#"
        const K = {{ D: "KHR_d", U: "KHR_u" }};
        class Draco {{ decodePrimitive(p: any, q: any): any {{ return p; }} }}
        class Unlit {{ name = K.U; }}
        class P {{ go(): number {{ return 1; }} }}
        const u: any = (globalThis as any)[process.argv[2]];
        u[process.argv[3]] = process.argv[4];
        const ext: any = {{}};
        const pl = new Unlit();
        ext[pl.name] = true;
        for (const n of process.argv.slice(5)) {{
            if (n === K.D) ext[n] = new Draco();
        }}
        const pp = new P();
        {call}
        console.log(pp.go());
        "#
    )
}

#[test]
fn computed_read_on_a_literal_hands_no_argument_to_a_written_method() {
    let pf = facts(&program("ext[K.D].decodePrimitive(pp, 0);"));
    let v = sites(&pf, "go");
    assert!(proven(&v), "{v:?}\n{}", pf.dump_text());
}

#[test]
fn control_method_written_by_unknown_code_on_a_string() {
    let pf = facts(&program(
        "(String.prototype as any)[process.argv[3]] = new Function(process.argv[2])(); (process.argv[2] as any).decodePrimitive(pp, 0);",
    ));
    assert!(!proven(&sites(&pf, "go")));
}

/// Corrected (was a control asserting the escape): `JSON.parse` makes no
/// function (ECMA-262 25.5.1), and the key a store into ⊤ may have written
/// on the chain holds the stored string: the call throws, `pp` reaches no
/// one.
#[test]
fn receiver_parsed_from_the_command_line_calls_no_function() {
    let pf = facts(&program("JSON.parse(process.argv[2]).decodePrimitive(pp, 0);"));
    assert!(proven(&sites(&pf, "go")), "{:?}", sites(&pf, "go"));
}

/// Without the store into ⊤ nothing may be written on the chain: proven
/// (isolates the writer of the red test above).
#[test]
fn without_the_top_store_the_argument_stays() {
    let src = program("ext[K.D].decodePrimitive(pp, 0);").replace("u[process.argv[3]] = process.argv[4];", "");
    let pf = facts(&src);
    assert!(proven(&sites(&pf, "go")), "{:?}", sites(&pf, "go"));
}

/// The ⊤ store stored a function: a call of the key on a string calls it,
/// with `pp` as its argument (its own call of `fly` on it is proven).
#[test]
fn a_stored_function_is_called_with_the_argument() {
    let src = program(
        "const r = (process.argv[2] as any).decodePrimitive(pp, 0); console.log(r.fly());",
    )
    .replace(
        "u[process.argv[3]] = process.argv[4];",
        "u[process.argv[3]] = function (x: any, y: any) { x.fly(); return x; };",
    )
    .replace("go(): number { return 1; }", "go(): number { return 1; } fly(): number { return 2; }");
    let pf = facts(&src);
    let go = sites(&pf, "go");
    let fly = sites(&pf, "fly");
    assert!(proven(&go) && proven(&fly), "{go:?} {fly:?}\n{}", pf.dump_text());
}

/// The ⊤ store stored what unknown code made: the call runs unknown code.
#[test]
fn control_stored_value_made_by_unknown_code() {
    let src = program("(process.argv[2] as any).decodePrimitive(pp, 0);").replace(
        "u[process.argv[3]] = process.argv[4];",
        "u[process.argv[3]] = new Function(process.argv[2])();",
    );
    assert!(!proven(&sites(&facts(&src), "go")));
}

/// A getter defined through ⊤ (`Object.defineProperty`): the key's value is the getter's result, not
/// modelled here (opaque): the argument escapes.
#[test]
fn control_getter_defined_through_top() {
    let src = program("(process.argv[2] as any).decodePrimitive(pp, 0);").replace(
        "u[process.argv[3]] = process.argv[4];",
        "Object.defineProperty(u, process.argv[3], { get: function () { return 1; } });",
    );
    assert!(!proven(&sites(&facts(&src), "go")));
}

/// A delete through ⊤ stores nothing: the key reads undefined, the call
/// throws, nothing escapes.
#[test]
fn a_delete_through_top_hands_no_argument() {
    let src = program("ext[K.D].decodePrimitive(pp, 0); (process.argv[2] as any).decodePrimitive(pp, 0);")
        .replace("u[process.argv[3]] = process.argv[4];", "delete u.decodePrimitive;");
    let pf = facts(&src);
    assert!(proven(&sites(&pf, "go")), "{:?}\n{}", sites(&pf, "go"), pf.dump_text());
}
