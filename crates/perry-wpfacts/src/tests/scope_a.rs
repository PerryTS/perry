//! SCOPEA (DESIGN_SCOPE64 choice (a)): sites the solver already describes
//! exactly, proven by an existing verdict once the receiver set is read:
//! a RegExp's read-only getters, a Number primitive's `Number.prototype`
//! member, a native Promise's `then`/`catch`/`finally`. Each item has a control that must stay unproven: the write
//! (or the receiver) that makes the site's outcome depend on more than the
//! intrinsic.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The verdicts of the sites of key `key` (`call`: method calls, else
/// reads), in source order.
fn verdicts(pf: &ProgramFacts, key: &str, call: bool) -> Vec<Verdict> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter().map(|s| s.verdict.clone()).collect()
}

fn all(src: &str, key: &str, call: bool, want: Verdict) {
    let pf = one(src);
    let v = verdicts(&pf, key, call);
    assert!(
        !v.is_empty() && v.iter().all(|v| *v == want),
        "{key} call={call}: want {want:?}, got {v:?}\n{src}\n{}",
        pf.dump_text()
    );
}

fn none(src: &str, key: &str, call: bool) {
    let pf = one(src);
    let v = verdicts(&pf, key, call);
    assert!(
        !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_))),
        "{key} call={call}: want unproven, got {v:?}\n{src}\n{}",
        pf.dump_text()
    );
}

const N: Verdict = Verdict::NativeMember;

// b. RegExp.prototype's read-only getters (22.2.6.4 flags, .5 global,
// .13 source, .14 sticky): accessors without a setter on the intrinsic
// prototype that read only the receiver's internal slots. `flags` (.4)
// reads the flag keys through [[Get]]: not proven by its key alone.
const RE: &str = "function main() { const re = /ab+c/g; \
    return re.source + String(re.global) + String(re.sticky); }\nconsole.log(main());";

#[test]
fn b_regexp_getters_proven() {
    all(RE, "source", false, N);
    all(RE, "sticky", false, N);
    all(RE, "global", false, N);
}

#[test]
fn b_regexp_own_source_not_proven() {
    let src = "function main() { const re = /ab+c/g; \
        Object.defineProperty(re, \"source\", { value: \"x\" }); \
        return re.source; }\nconsole.log(main());";
    none(src, "source", false);
}

#[test]
fn b_regexp_subclass_not_proven() {
    let src = "class R extends RegExp { get source() { return \"x\"; } }\n\
        function main() { const re: any = new R(\"a\"); return re.source; }\nconsole.log(main());";
    none(src, "source", false);
}

// c. A Number primitive's member is %Number.prototype%'s (7.3.2 GetV:
// ToObject, then [[Get]] on the wrapper's chain; a primitive owns nothing).
const NUM: &str = "function main() { const n = Number(process.argv.length) * 1.5; \
    return n.toFixed(2); }\nconsole.log(main());";

#[test]
fn c_number_to_fixed_proven() {
    all(NUM, "toFixed", true, N);
}

/// DEC 250: `Number.prototype` refuses replacing an image member (the
/// store throws a TypeError and writes nothing), so the read stays final.
#[test]
fn c_number_proto_store_refused_still_proven() {
    let src = format!("(Number.prototype as any).toFixed = function () {{ return \"x\"; }};\n{NUM}");
    all(&src, "toFixed", true, N);
}

/// A Number or an object: no one family; the site keeps the verdict it
/// had (the object's own method is the call set's body).
#[test]
fn c_number_or_object_not_native() {
    let src = "function main() { const n: any = process.argv.length > 2 ? Number(process.argv[2]) \
        : { toFixed(d: number) { return \"b\"; } }; return n.toFixed(2); }\nconsole.log(main());";
    let pf = one(src);
    let v = verdicts(&pf, "toFixed", true);
    assert!(!v.is_empty() && !v.contains(&N), "{v:?}");
}

#[test]
fn c_number_or_string_not_proven() {
    let src = "function main() { const n: any = process.argv.length > 2 ? Number(process.argv[2]) \
        : String(process.argv[1]); return n.toFixed(2); }\nconsole.log(main());";
    none(src, "toFixed", true);
}

// f. A native Promise's then/catch/finally (27.2.5.1/.3/.4 on
// %Promise.prototype%).
const PROM: &str = "function main() { const p = Promise.resolve(Number(process.argv.length)); \
    return p.then((x: number) => x + 1).catch(() => 0).finally(() => {}); }\nmain();";

#[test]
fn f_promise_members_proven() {
    all(PROM, "then", true, N);
    all(PROM, "catch", true, N);
    all(PROM, "finally", true, N);
}

#[test]
fn f_thenable_not_proven() {
    let src = "function main() { const p: any = process.argv.length > 5 ? Promise.resolve(1) \
        : { then(f: any) { return f(1); } }; return p.then((x: number) => x + 1); }\nmain();";
    none(src, "then", true);
}

#[test]
fn f_promise_subclass_not_proven() {
    let src = "class P<T> extends Promise<T> { then(a?: any, b?: any): any { return 7; } }\n\
        function main() { const p: any = new P<number>((r) => r(1)); return p.then((x: number) => x); }\nmain();";
    none(src, "then", true);
}
