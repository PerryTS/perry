//! CHW4 rule HI: `x instanceof C` with `C` a class whose `extends` is a
//! value, or a plain function. The lookup of `@@hasInstance` on such a
//! constructor passes its own keys and those of its parents (program
//! tokens, read by the chain walk), then reaches `%Function.prototype%`,
//! whose `@@hasInstance` is non-writable and non-configurable (ECMA-262
//! 20.2.3.6) and an image member the runtime refuses to replace
//! (DEC 250). A plain store of the key on the constructor or a parent
//! fails against that inherited property (10.1.9.2 step 2.a); only a define
//! installs one, as an own key the walk reads. So the intrinsic end of the
//! lookup is final: a store through an unknown key, with the global object
//! given to unknown code, does not make the call unknown. A parent ⊤ (a
//! value unknown code supplies, a `setPrototypeOf` to one) still gives the
//! unknown method through the walk, and an escaped constructor still takes
//! what unknown code defines on it.
use super::*;

fn facts(src: &str) -> crate::ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    assert!(pf.body_reach.complete, "activated");
    pf
}

/// The verdict of the one `p.f()` call in `probe`.
fn probe_call(pf: &crate::ProgramFacts) -> Verdict {
    let calls: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "f" && s.is_call)
        .collect();
    assert_eq!(calls.len(), 1, "{}", pf.dump_text());
    calls[0].verdict.clone()
}

/// The writers of the chess-headless solve: the global object given to
/// unknown code, and a store through an unknown key into
/// `globalThis.globalThis`, which unknown code may have written (a store
/// into ⊤).
const WRITERS: &str = r#"const unknown: any = new Function(process.argv[2])();
unknown.take2(globalThis);
const G: any = (globalThis as any).globalThis;
G[process.argv[3]] = unknown.v;
function sink(x: any) { unknown.take3(x); }"#;

/// `o` is the left-hand side; `probe(o)` calls `o.f`.
const PROBE: &str = r#"const o: any = { f() { return 1; } };
function probe(p: any) { return p.f(); }
console.log(probe(o), o instanceof RHS, probe(o));"#;

/// A class whose `extends` is a value (E6 `parent_value`).
const VALUE_EXTENDS: &str = r#"class B {}
class C2 {}
const P: any = process.argv.length > 9 ? B : C2;
class A extends P {}"#;

fn run(head: &str, rhs: &str) -> crate::ProgramFacts {
    facts(&format!("{WRITERS}\n{head}\n{}", PROBE.replace("RHS", rhs)))
}

/// RED: `instanceof` a value-extends class runs OrdinaryHasInstance: the
/// left-hand side does not escape, the call stays monomorphic.
#[test]
fn chw4_value_extends_class_has_instance_is_final() {
    let pf = run(VALUE_EXTENDS, "A");
    assert_eq!(probe_call(&pf), Verdict::T0Mono, "{}", pf.dump_text());
}

/// RED: a program subclass of a value-extends class: its `extends` chain
/// passes the value parent too.
#[test]
fn chw4_subclass_of_value_extends_class_has_instance_is_final() {
    let pf = run(&format!("{VALUE_EXTENDS}\nclass D extends A {{}}"), "D");
    assert_eq!(probe_call(&pf), Verdict::T0Mono, "{}", pf.dump_text());
}

/// KEEP (green before HI as well): a plain function is born with
/// `%Function.prototype%` (`born_with_function_proto`), whose read was
/// final already; the same writers leave the call monomorphic.
#[test]
fn chw4_keep_plain_function_has_instance_is_final() {
    let pf = run("const F: any = function (this: any) { this.q = 1; };", "F");
    assert_eq!(probe_call(&pf), Verdict::T0Mono, "{}", pf.dump_text());
}

/// KEEP: a program-written `static [Symbol.hasInstance]` on the class runs
/// with the left-hand side.
#[test]
fn chw4_keep_static_has_instance_runs() {
    let pf = run(
        "class B {}\nclass C2 {}\nconst P: any = process.argv.length > 9 ? B : C2;\nclass A extends P { static [Symbol.hasInstance](v: any) { v.f = function () { return 7; }; return true; } }",
        "A",
    );
    assert_ne!(probe_call(&pf), Verdict::T0Mono, "{}", pf.dump_text());
}

/// KEEP: a program `Object.defineProperty` of the key on the value parent
/// is on the chain: it runs with the left-hand side.
#[test]
fn chw4_keep_defined_on_parent_runs() {
    let pf = run(
        &format!("{VALUE_EXTENDS}\nObject.defineProperty(B, Symbol.hasInstance, {{ value: (v: any) => {{ v.f = function () {{ return 7; }}; return true; }} }});"),
        "A",
    );
    assert_ne!(probe_call(&pf), Verdict::T0Mono, "{}", pf.dump_text());
}

/// KEEP: unknown code that holds `A` may define `A[@@hasInstance]`: the
/// left-hand side escapes.
#[test]
fn chw4_keep_sunk_class_escapes() {
    let pf = run(&format!("{VALUE_EXTENDS}\nsink(A);"), "A");
    assert!(
        matches!(probe_call(&pf), Verdict::No(_)),
        "{}",
        pf.dump_text()
    );
}

/// KEEP: `setPrototypeOf(A, unknown)`: the lookup continues in unknown
/// code's object: the left-hand side escapes.
#[test]
fn chw4_keep_set_prototype_of_unknown_escapes() {
    let pf = run(
        &format!(
            "{VALUE_EXTENDS}\nObject.setPrototypeOf(A, unknown.p);"
        ),
        "A",
    );
    assert!(
        matches!(probe_call(&pf), Verdict::No(_)),
        "{}",
        pf.dump_text()
    );
}

/// KEEP: a parent value unknown code supplies is ⊤ on the chain.
#[test]
fn chw4_keep_unknown_parent_escapes() {
    let pf = run(
        "const P: any = unknown.p;\nclass A extends P {}",
        "A",
    );
    assert!(
        matches!(probe_call(&pf), Verdict::No(_)),
        "{}",
        pf.dump_text()
    );
}

/// KEEP: unknown code that holds a plain function may define its
/// `@@hasInstance`.
#[test]
fn chw4_keep_sunk_function_escapes() {
    let pf = run(
        "const F: any = function (this: any) { this.q = 1; };\nsink(F);",
        "F",
    );
    assert!(
        matches!(probe_call(&pf), Verdict::No(_)),
        "{}",
        pf.dump_text()
    );
}

/// KEEP: a builtin constructor as the parent value: unknown code that holds
/// the global object may define `Error[@@hasInstance]`, which the lookup on
/// `A` reaches: the left-hand side escapes.
#[test]
fn chw4_keep_builtin_parent_value_escapes() {
    let pf = run(
        "const P: any = process.argv.length > 9 ? Error : TypeError;\nclass A extends P {}",
        "A",
    );
    assert!(
        matches!(probe_call(&pf), Verdict::No(_)),
        "{}",
        pf.dump_text()
    );
}
