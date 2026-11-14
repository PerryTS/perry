//! PRIMRECV: a member read or method call whose receiver set holds only
//! primitives (no object token but closed markers, no ⊤).
//! (N1) The set holds only `undefined`/`null`: the member access evaluates
//! its base, then GetValue's ToObject (ECMA-262 13.3.2.1, 6.2.5.5, 7.1.18)
//! throws a TypeError before any property lookup: `Throws`.
//! (N2) `undefined`/`null` beside the primitives of one family (String or
//! Number): the operation's own dispatch on the value's kind runs the
//! family's intrinsic member (or a String's own `length`) for the
//! primitive and throws the TypeError for the nullish value. The claim is
//! NativeMember / OwnLength naming both arms.
//! String/Number prototypes are locked against replacement of an existing
//! member (perry-abi lock.rs:16), so a held realm keeps an EXISTING member's
//! claim; a key absent on the chain is not exact under a held realm and
//! stays open, as does a set of two families (`toString` of a string or a
//! number names two intrinsics).
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// Verdicts of the sites of `key` (calls when `call`), in source order.
fn at(pf: &ProgramFacts, key: &str, call: bool) -> Vec<String> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && s.is_call == call).collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?} {:?}", s.verdict, s.receivers)).collect();
    eprintln!("LITFACT {key} call={call}: {v:?}");
    v.into_iter().map(|v| v.split(' ').next().unwrap_or("").to_string()).collect()
}

fn all(v: &[String], k: &str) -> bool {
    !v.is_empty() && v.iter().all(|v| v == k)
}
fn none(v: &[String], k: &str) -> bool {
    !v.is_empty() && v.iter().all(|v| v != k)
}

const C: &str = "const c = Math.random() > 0.5;\nconst d = Math.random() > 0.5;\n";

#[test]
fn s1_string_only_member_call_is_native() {
    let pf = one(&format!("{C}const s: any = c ? \"ab\" : \"cd\";\nconsole.log(s.replace(\"a\", \"b\"));\n"));
    assert!(all(&at(&pf, "replace", true), "NativeMember"));
}

#[test]
fn s2_string_only_length_is_own_length() {
    let pf = one(&format!("{C}const s: any = c ? \"ab\" : \"cde\";\nconsole.log(s.length);\n"));
    assert!(all(&at(&pf, "length", false), "OwnLength"));
}

#[test]
fn n2_string_or_undefined_member_call() {
    let pf = one(&format!(
        "{C}const s: any = c ? \"ab\" : undefined;\nconsole.log(s.replace(\"a\", \"b\"), s.startsWith(\"a\"));\n"
    ));
    assert!(all(&at(&pf, "replace", true), "NativeMember"));
    assert!(all(&at(&pf, "startsWith", true), "NativeMember"));
}

#[test]
fn n2_string_or_null_length() {
    let pf = one(&format!("{C}const s: any = c ? \"ab\" : null;\nconsole.log(s.length);\n"));
    assert!(all(&at(&pf, "length", false), "OwnLength"));
}

#[test]
fn n2_number_or_undefined_member_call() {
    let pf = one(&format!("{C}const n: any = c ? 1.5 : undefined;\nconsole.log(n.toFixed(1));\n"));
    assert!(all(&at(&pf, "toFixed", true), "NativeMember"));
}

#[test]
fn n1_undefined_only_read_and_call() {
    let pf = one(&format!(
        "{C}let u: any = undefined;\nif (d) console.log(u.view, u.go(1));\n"
    ));
    assert!(all(&at(&pf, "view", false), "Throws"));
    assert!(all(&at(&pf, "go", true), "Throws"));
}

#[test]
fn n1_null_or_undefined_only() {
    let pf = one(&format!(
        "{C}let u: any = c ? null : undefined;\nif (d) console.log(u.enabled);\n"
    ));
    assert!(all(&at(&pf, "enabled", false), "Throws"));
}

#[test]
fn control_held_realm_keeps_an_existing_locked_member() {
    // Unknown code holds the realm (9.3) but cannot replace an existing
    // String.prototype member: the runtime refuses it (perry-abi lock.rs:16).
    let pf = one(&format!(
        "{C}declare function sink(x: any): void;\nsink(0);\n\
         const s: any = c ? \"ab\" : undefined;\nconsole.log(s.replace(\"a\", \"b\"));\n"
    ));
    assert!(all(&at(&pf, "replace", true), "NativeMember"));
}

#[test]
fn control_object_with_own_member_beside_string_and_undefined() {
    let pf = one(&format!(
        "{C}const o = {{ replace(a: any, b: any) {{ return 1; }} }};\n\
         const s: any = c ? \"ab\" : d ? undefined : o;\nconsole.log(s.replace(\"a\", \"b\"));\n"
    ));
    assert!(none(&at(&pf, "replace", true), "NativeMember"));
}

#[test]
fn absent_key_on_string_or_undefined_read_claimed_call_open() {
    let pf = one(&format!(
        "{C}const s: any = c ? \"ab\" : undefined;\nconsole.log(s.isColor, s.getHex());\n"
    ));
    // Shape A (PRIMRECV3): the read is the absent claim with the TypeError
    // arm; the call stays open (Throws is before any property lookup).
    assert!(all(&at(&pf, "isColor", false), "OwnDataOrAbsent"));
    let k = at(&pf, "getHex", true);
    assert!(none(&k, "NativeMember") && none(&k, "Throws") && none(&k, "OwnDataOrAbsent"), "{k:?}");
}

#[test]
fn control_string_and_number_name_two_intrinsics() {
    let pf = one(&format!(
        "{C}const s: any = c ? \"ab\" : d ? 1 : undefined;\nconsole.log(s.toString());\n"
    ));
    assert!(none(&at(&pf, "toString", true), "NativeMember"));
}

#[test]
fn control_undefined_beside_an_object_does_not_throw() {
    let pf = one(&format!(
        "{C}const u: any = c ? undefined : {{ view: 1 }};\nconsole.log(u.view);\n"
    ));
    assert!(none(&at(&pf, "view", false), "Throws"));
}

#[test]
fn control_unknown_receiver_does_not_throw() {
    // A declared function is unknown code: its result is ⊤ beside `undefined`.
    // (A `declare const` with no unseen module is rule G's absent global,
    // `undefined`: no control.) An unseen module may write any global
    // (rule G, `super::unseen`).
    for src in [
        format!("{C}declare function src(): any;\nlet u: any = undefined;\nif (d) u = src();\nconsole.log(u.view);\n"),
        super::unseen(&format!("{C}declare const g: any;\nlet u: any = undefined;\nif (d) u = g;\nconsole.log(u.view);\n")),
    ] {
        let pf = one(&src);
        assert!(none(&at(&pf, "view", false), "Throws"));
    }
}

#[test]
fn control_optional_chain_does_not_throw() {
    let pf = one(&format!("{C}let u: any = undefined;\nconsole.log(u?.view, u?.go(1));\n"));
    let r = at(&pf, "view", false);
    let k = at(&pf, "go", true);
    assert!(r.iter().chain(k.iter()).all(|v| v != "Throws"), "{r:?} {k:?}");
}

/// PRIMRECV2 step 1: a pure primitive set whose source node left a value
/// out (`dropped`: the parameter marker of `outer` crossed into `inner`
/// and was replaced by `inner`'s argument marker) is not the whole truth:
/// no NativeMember / OwnLength, as for the two-kind and Throws claims.
#[test]
fn dropped_source_refuses_native_member_and_own_length() {
    let src = format!(
        "{C}function inner(y: any) {{ return y.replace(\"a\", \"b\") + y.length; }}\n\
         function outer(x: any) {{ return inner(x); }}\n\
         console.log(outer(c ? \"ab\" : \"cd\"));\n"
    );
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(&src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let pf = crate::compute_units(&[(path, &hir)], &FactsConfig::default());
    assert!(none(&at(&pf, "replace", true), "NativeMember"));
    assert!(none(&at(&pf, "length", false), "OwnLength"));
}

/// PRIMRECV2 step 2: an undeclared global reads `undefined` (rule G) only
/// while no unknown code holds the realm; held, unknown code may define the
/// global key (`globalThis.g = …`), so the call is not Throws.
#[test]
fn undeclared_global_under_held_realm_does_not_throw() {
    let held = one(&format!(
        "{C}declare const g: any;\ndeclare function hostTake(x: any): void;\nhostTake(0);\nif (d) g.m();\n"
    ));
    assert!(none(&at(&held, "m", true), "Throws"));
    let unheld = one(&format!("{C}declare const g: any;\nif (d) g.m();\n"));
    assert!(all(&at(&unheld, "m", true), "Throws"));
}

/// PRIMRECV2 step 2 (three.core.js:60566 shape): `typeof g !== 'undefined'
/// && g.m()`; the verdict is printed (LITFACT line), not asserted.
#[test]
fn typeof_guarded_undeclared_global_verdict() {
    for hold in ["", "declare function hostTake(x: any): void;\nhostTake(0);\n"] {
        let pf = one(&format!(
            "{C}declare const g: any;\n{hold}if (typeof g !== 'undefined') g.m();\n"
        ));
        let _ = at(&pf, "m", true);
    }
}

// ---------------------------------------------------------------- PRIMRECV3

/// PRIMRECV3 step 1: `get RegExp.prototype.source` (ECMA-262 22.2.6.13) of a
/// RegExp object reads its [[OriginalSource]] slot (EscapeRegExpPattern):
/// always a String. `RegExp.prototype`'s `source` is an image member, which
/// the lock refuses to every mutation, so the getter is the one that runs.
#[test]
#[ignore = "PRIMRECV3 step 1 open: load_from seeds undefined for a key no sure own data holds (solve/lookup.rs `!present` arm) before the RegExp getter's String"]
fn regexp_source_of_a_regexp_object_is_a_string() {
    let exact = |pf: &ProgramFacts| {
        pf.sites
            .iter()
            .filter(|s| s.key == "replace")
            .all(|s| s.receivers == vec!["primitive:string".to_string()])
    };
    let pf = one(&format!(
        "{C}const r = c ? /a/ : /b+/;\nconsole.log(r.source.replace(\"a\", \"b\"));\n"
    ));
    assert!(all(&at(&pf, "replace", true), "NativeMember") && exact(&pf));
    let pf = one(&format!(
        "{C}function mk(p: string) {{ return new RegExp(p, \"g\"); }}\n\
         const r = c ? mk(\"a\") : mk(\"b\");\nconsole.log(r.source.replace(\"a\", \"b\"));\n"
    ));
    assert!(all(&at(&pf, "replace", true), "NativeMember") && exact(&pf));
}

/// Control: the receiver may be `%RegExp.prototype%` itself, which has no
/// [[OriginalSource]] (22.2.6.13 step 3): not this rule.
#[test]
fn control_regexp_source_of_the_prototype_itself() {
    let pf = one(&format!(
        "{C}const r: any = c ? /a/ : RegExp.prototype;\nconsole.log(r.source.replace(\"a\", \"b\"));\n"
    ));
    assert!(none(&at(&pf, "replace", true), "NativeMember"));
}

/// Control: an own `source` (a literal beside it, or one defined on the
/// RegExp object) is what the read finds, not the getter.
#[test]
fn control_own_source_is_not_the_getter() {
    let pf = one(&format!(
        "{C}const r: any = c ? /a/ : {{ source: 1 }};\nconsole.log(r.source.replace(\"a\", \"b\"));\n"
    ));
    assert!(none(&at(&pf, "replace", true), "NativeMember"));
    let pf = one(&format!(
        "{C}const r: any = /a/;\nObject.defineProperty(r, \"source\", {{ value: 1 }});\n\
         console.log(r.source.replace(\"a\", \"b\"));\n"
    ));
    assert!(none(&at(&pf, "replace", true), "NativeMember"));
}

const HOLD: &str = "declare function hostTake(x: any): void;\nhostTake(0);\n";

/// Shape A: a key no family prototype of the primitives (nor
/// `%Object.prototype%`) holds and the program never writes there:
/// GetV's ToObject wrapper owns no such key (a String wrapper owns its
/// indices and `length` only, 10.4.3), so OrdinaryGet gives `undefined`.
#[test]
fn a_absent_key_on_strings() {
    let pf = one(&format!("{C}const s: any = c ? \"a\" : \"b\";\nconsole.log(s.isStackTrace);\n"));
    assert!(all(&at(&pf, "isStackTrace", false), "OwnDataOrAbsent"));
}

#[test]
fn a_absent_key_across_three_families() {
    let pf = one(&format!(
        "{C}const x: any = c ? \"a\" : d ? 1 : true;\nconsole.log(x.isStackTrace);\n"
    ));
    assert!(all(&at(&pf, "isStackTrace", false), "OwnDataOrAbsent"));
}

/// Nullish beside the primitives: the TypeError arm is named.
#[test]
fn a_absent_key_beside_nullish_names_the_type_error_arm() {
    let pf = one(&format!("{C}const s: any = c ? \"a\" : undefined;\nconsole.log(s.isStackTrace);\n"));
    assert!(all(&at(&pf, "isStackTrace", false), "OwnDataOrAbsent"));
    assert!(pf
        .sites
        .iter()
        .filter(|s| s.key == "isStackTrace")
        .all(|s| s.receivers.iter().any(|r| r.ends_with("(TypeError)"))));
}

#[test]
fn control_absent_key_held_realm() {
    let pf = one(&format!("{C}{HOLD}const s: any = c ? \"a\" : \"b\";\nconsole.log(s.isStackTrace);\n"));
    assert!(none(&at(&pf, "isStackTrace", false), "OwnDataOrAbsent"));
}

#[test]
fn control_absent_key_written_on_a_prototype() {
    for w in ["(String.prototype as any).isStackTrace = 1;", "(Object.prototype as any).isStackTrace = 1;"] {
        let pf = one(&format!(
            "{C}{w}\nconst s: any = c ? \"a\" : \"b\";\nconsole.log(s.isStackTrace);\n"
        ));
        assert!(none(&at(&pf, "isStackTrace", false), "OwnDataOrAbsent"), "{w}");
    }
}

#[test]
fn control_absent_shape_needs_an_absent_key() {
    // `length` of a String, an index, and a member of the chain.
    let pf = one(&format!(
        "{C}const x: any = c ? \"a\" : 1;\nconsole.log(x.length, x[0], x.toString);\n"
    ));
    assert!(none(&at(&pf, "length", false), "OwnDataOrAbsent"));
    assert!(none(&at(&pf, "toString", false), "OwnDataOrAbsent"));
}

/// A call of an absent member: printed, left open (Throws names a TypeError
/// before any property lookup; this one is after the arguments).
#[test]
fn a_call_of_an_absent_member_verdict() {
    let pf = one(&format!("{C}const s: any = c ? \"a\" : \"b\";\nconsole.log(s.getLocation(1));\n"));
    assert!(none(&at(&pf, "getLocation", true), "Throws"));
}
