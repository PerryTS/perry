//! NMHELD: the verdict kinds that claim a builtin member, under a held
//! realm. Unknown code holds the realm (ECMA-262 9.3) once it runs
//! (`Model::realm_held`): it may replace every member a store can reach.
//! A claim "this site reaches intrinsic member M" is exact under a held
//! realm only when no store can replace M: the runtime refuses it (DEC 250,
//! `members::store_admitted`: an image member of a locked prototype). Each
//! held fixture runs unknown code (`sink(0)`, the shape of NEWVERD6's
//! held-realm control); its twin without unknown code keeps the claim.
use super::*;

const HELD: &str = "declare function sink(x: any): void;\nsink(0);\n";

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The verdicts of the sites of key `key` (`call`: calls), in source order.
fn verdicts(src: &str, key: &str, call: bool) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("NMHELD {key} call={call}: {v:?}");
    assert!(!v.is_empty(), "no site of {key}");
    v
}

fn claimed(v: &[String], kind: &str) -> bool {
    v.iter().any(|x| x == kind || x.starts_with(&format!("{kind}(")))
}

fn both(body: &str, key: &str, call: bool) -> (Vec<String>, Vec<String>) {
    (
        verdicts(body, key, call),
        verdicts(&format!("{HELD}{body}"), key, call),
    )
}

// Image members of locked prototypes: the runtime refuses their
// replacement, unknown code's store included, so the claim stays exact.
const PROMISE_THEN: &str =
    "async function g() { return 1; }\nconst p = g();\nconst q = p.then((x: number) => x + 1);";
const REGEXP_TEST: &str = "const re = /a/;\nconst b = re.test(\"abc\");";
const NUMBER_TOFIXED: &str = "const n = 1.5;\nconst s = n.toFixed(2);";
const ARRAY_SHIFT: &str = "function f(...r: any) { return r.shift(); }\nf(1);";

#[test]
fn native_member_locked_promise_then() {
    let (free, held) = both(PROMISE_THEN, "then", true);
    assert!(claimed(&free, "NativeMember"), "{free:?}");
    assert!(claimed(&held, "NativeMember"), "{held:?}");
}

#[test]
fn native_member_locked_regexp_test() {
    let (free, held) = both(REGEXP_TEST, "test", true);
    assert!(claimed(&free, "NativeMember"), "{free:?}");
    assert!(claimed(&held, "NativeMember"), "{held:?}");
}

#[test]
fn native_member_locked_number_tofixed() {
    let (free, held) = both(NUMBER_TOFIXED, "toFixed", true);
    assert!(claimed(&free, "NativeMember"), "{free:?}");
    assert!(claimed(&held, "NativeMember"), "{held:?}");
}

#[test]
fn native_member_locked_array_shift() {
    let (free, held) = both(ARRAY_SHIFT, "shift", true);
    assert!(claimed(&free, "NativeMember"), "{free:?}");
    assert!(claimed(&held, "NativeMember"), "{held:?}");
}

// Members the runtime lets a store replace: unknown code holding the realm
// may have replaced them, a write the solve does not replay.
const BUFFER_TOSTRING: &str = "const buf = Buffer.from(\"ab\");\nconst s = buf.toString();";
const CONSOLE_LOG: &str = "console.log(\"x\");";

#[test]
fn native_member_buffer_free() {
    let (free, _) = both(BUFFER_TOSTRING, "toString", true);
    assert!(claimed(&free, "NativeMember"), "{free:?}");
}

#[test]
fn native_member_buffer_held_not_claimed() {
    let (_, held) = both(BUFFER_TOSTRING, "toString", true);
    assert!(!claimed(&held, "NativeMember"), "{held:?}");
}

#[test]
fn native_member_host_free() {
    let (free, _) = both(CONSOLE_LOG, "log", true);
    assert!(claimed(&free, "NativeMember"), "{free:?}");
}

#[test]
fn native_member_host_held_not_claimed() {
    let (_, held) = both(CONSOLE_LOG, "log", true);
    assert!(!claimed(&held, "NativeMember"), "{held:?}");
}

// T0Mono names a program body; unknown code holds the realm, not the
// program's objects it was never given.
const CLASS_CALL: &str = "class A { m() { return 1; } }\nconst a = new A();\nconst r = a.m();";

#[test]
fn t0mono_program_body_free_and_held() {
    let (free, held) = both(CLASS_CALL, "m", true);
    assert!(claimed(&free, "T0Mono"), "{free:?}");
    assert!(claimed(&held, "T0Mono"), "{held:?}");
}

#[test]
fn t0mono_control_object_given_to_unknown_code() {
    let src = format!("{HELD}{CLASS_CALL}\nsink(a);\nconst r2 = a.m();");
    let v = verdicts(&src, "m", true);
    assert!(!claimed(&v, "T0Mono"), "{v:?}");
}
