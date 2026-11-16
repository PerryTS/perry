//! KJ1: the OPTIMISTIC JSON-key rule (owner 2026-11-12, measurement only,
//! default off; `FactsConfig::optimistic_json_key`,
//! `PERRY_FACTS_OPTIMISTIC_JSON_KEY=1`, `Model::json_key_optimistic`). A
//! computed key on a `JSON.parse` object or a program literal / Array is
//! assumed to name no built-in member. Every fixture runs both ways: with
//! the switch off (the named control) it is today's solve.
use super::*;

fn run(src: &str, on: bool) -> ProgramFacts {
    facts_with(
        src,
        FactsConfig {
            optimistic_json_key: on,
            ..FactsConfig::default()
        },
    )
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && s.is_call).collect();
    assert!(!s.is_empty(), "no call .{key}\n{}", pf.dump_text());
    s.iter().all(|s| s.verdict == Verdict::T0Mono)
}

const CLASSES: &str = r#"
class A { m() { return 1; } }
class B { n() { return 2; } }
class D { d() { return 3; } }
"#;

fn table(key: &str) -> String {
    format!(
        "{CLASSES} const j = JSON.parse(process.argv[2]); const table = [new A(), new A()]; console.log(table[{key}].m());"
    )
}

fn reg(key: &str, recv: &str) -> String {
    format!(
        "{CLASSES} const j = JSON.parse(process.argv[2]); const reg: any = {recv}; reg[{key}] = new B(); console.log(reg[{key}2].n());"
    )
}

const FORIN: &str = r#"
const j = JSON.parse(process.argv[2]);
const out: any = {};
for (const k in j.ext) out[k] = j.ext[k];
out[j.slot] = new D();
console.log(out[j.slot2].d());
"#;

#[test]
fn json_index_into_literal_array() {
    assert!(proven(&run(&table("j.i"), true), "m"));
}

#[test]
fn control_off_json_index_into_literal_array() {
    assert!(!proven(&run(&table("j.i"), false), "m"));
}

#[test]
fn json_key_into_object_literal() {
    assert!(proven(&run(&reg("j.name", "{}"), true), "n"));
}

#[test]
fn control_off_json_key_into_object_literal() {
    assert!(!proven(&run(&reg("j.name", "{}"), false), "n"));
}

/// The receivers of the `.d` call: the copied json values and `D`; a
/// built-in member (`builtin:...`) only with the switch off. (The call
/// stays unproven both ways: the json values copied into `out` are real.)
fn d_recv_builtin(pf: &ProgramFacts) -> bool {
    let d = pf.dump_text();
    let line = d
        .lines()
        .find(|l| l.starts_with("S ") && l.contains(" .d call=1"))
        .unwrap_or_else(|| panic!("no .d call\n{d}"))
        .to_string();
    line.contains("builtin:")
}

#[test]
fn for_in_copy_of_json_keys() {
    let pf = run(&format!("{CLASSES}{FORIN}"), true);
    assert!(!d_recv_builtin(&pf), "{}", pf.dump_text());
}

#[test]
fn control_off_for_in_copy_of_json_keys() {
    assert!(d_recv_builtin(&run(&format!("{CLASSES}{FORIN}"), false)));
}

/// Control: a receiver from unknown code is never covered.
#[test]
fn control_receiver_from_new_function() {
    let src = format!(
        "{CLASSES} const j = JSON.parse(process.argv[2]); const reg: any = new Function(process.argv[3])(); reg[j.name] = new B(); console.log(reg[j.name2].n());"
    );
    assert!(!proven(&run(&src, true), "n"));
}

/// Control: a compile-time constant key still reads the built-in member.
#[test]
fn control_constant_constructor_key() {
    let src = format!(
        "{CLASSES} const table: any = [new A(), new A()]; const c = \"constructor\"; console.log(table[c].m());"
    );
    assert!(!proven(&run(&src, true), "m"));
}

/// Form B sees the receiver, not the key's origin: a key read from
/// `process.argv` or returned by unknown code is covered as well (the
/// brief's form-A controls; measured, not a pass criterion of form B).
#[test]
fn form_b_argv_key_is_covered() {
    assert!(proven(&run(&table("process.argv[3]"), true), "m"));
    assert!(!proven(&run(&table("process.argv[3]"), false), "m"));
}

#[test]
fn form_b_new_function_key_is_covered() {
    let k = "new Function(process.argv[3])()";
    assert!(proven(&run(&table(k), true), "m"));
    assert!(!proven(&run(&table(k), false), "m"));
}
