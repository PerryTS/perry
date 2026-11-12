//! `ext[k].m(a)` on a plain object literal under a computed key: the value
//! is one of the literal`s fields or a member of `Object.prototype`. A store
//! into ⊤ (any key) cannot add a key to `Object.prototype` or replace one of
//! its members (DEC 250 lock), so the chain answer is final: no primitive
//! from a "written" chain member reaches the receiver, and `a` is handed to
//! no unknown method (three.js GLTFLoader `createDracoPrimitive`).
//! Controls: a method really written onto `String.prototype` by unknown code
//! called on a string keeps `a` escaping; so does a receiver parsed from
//! the command line.
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

#[test]
fn control_receiver_parsed_from_the_command_line() {
    let pf = facts(&program("JSON.parse(process.argv[2]).decodePrimitive(pp, 0);"));
    assert!(!proven(&sites(&pf, "go")));
}

/// Without the store into ⊤ nothing may be written on the chain: proven
/// (isolates the writer of the red test above).
#[test]
fn without_the_top_store_the_argument_stays() {
    let src = program("ext[K.D].decodePrimitive(pp, 0);").replace("u[process.argv[3]] = process.argv[4];", "");
    let pf = facts(&src);
    assert!(proven(&sites(&pf, "go")), "{:?}", sites(&pf, "go"));
}
