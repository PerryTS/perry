//! A call of "some `Object.prototype` method" (`BuiltinNs::ObjectProtoMethod`:
//! the value a computed key, or a key that may name any member, reads off
//! `Object.prototype`) runs each of those methods on the call's receiver,
//! the receiver node's values each on its own, as the named calls
//! `o.hasOwnProperty(k)`, `o.toString()` do. None of them hands `this` or
//! an argument to unknown code; the program code they run (a
//! `Symbol.toStringTag` getter, a program `toString`, a key's ToPrimitive)
//! still runs. A callee that really is unknown still escapes its argument.
use super::*;

const REASON: &str = "call of an Object.prototype method";

fn field_n(pf: &ProgramFacts) -> (&crate::derive::TemplateFacts, bool) {
    let t = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "n"))
        .unwrap_or_else(|| panic!("no template with field n\n{}", pf.dump_text()));
    let n = t.fields.iter().find(|f| f.key == "n").unwrap();
    (t, n.readonly)
}

fn no_reason(src: &str, pf: &ProgramFacts) {
    for t in &pf.templates {
        assert!(
            !t.escape.as_deref().is_some_and(|e| e.contains(REASON)),
            "{src}\n{} escaped: {:?}\n{}",
            t.display,
            t.escape,
            pf.dump_text()
        );
    }
}

/// The value kept its claims: no escape, `n` readonly.
fn kept(src: &str) {
    let pf = facts(src);
    no_reason(src, &pf);
    let (t, ro) = field_n(&pf);
    assert!(
        t.escape.is_none() && ro,
        "{src}\nescape={:?} readonly={ro}\n{}",
        t.escape,
        pf.dump_text()
    );
}

const KEY: &str = "const k: any = process.argv[2];\n";

/// The skin construct (three.js GLTFLoader.js:439 `json = JSON.parse(data)`,
/// then the value's string conversion): `Array.prototype.toString` on an
/// array-like `this` calls `this.join`, here "some Object.prototype method"
/// (a computed read), with `this` = the object.
#[test]
fn object_proto_method_as_array_like_join() {
    kept(&format!("{KEY}const o: any = {{ n: 1 }};\no.join = o[k];\nconsole.log(Array.prototype.toString.call(o), o.n);"));
}

/// The same method as the object's own `toString` / `valueOf`, run by
/// ToPrimitive.
#[test]
fn object_proto_method_as_to_primitive() {
    for m in ["toString", "valueOf"] {
        kept(&format!("{KEY}const o: any = {{ n: 1 }};\nconst p: any = {{}};\no.{m} = p[k];\nconsole.log(\"\" + o, `${{o}}`, String(o), o.n);"));
    }
}

/// Called as a method of the object it was read from.
#[test]
fn object_proto_method_on_receiver() {
    kept(&format!("{KEY}const o: any = {{ n: 1 }};\nconst r = o[k](\"n\");\no.m = o[k];\nconsole.log(r, o.m(\"n\"), o.n);"));
}

/// Given as the argument (a key: its ToPrimitive is Object.prototype's).
#[test]
fn object_proto_method_argument() {
    kept(&format!("{KEY}const o = {{ n: 1 }};\nconst p: any = {{}};\nconst q: any = {{ n2: 0 }};\nq.join = p[k];\nconsole.log(p[k](o), Array.prototype.toString.call(q), o.n);"));
}

/// Program code the methods run still runs with `this` = the receiver (or
/// the key): it writes `n`.
#[test]
fn object_proto_method_controls_run_program_code() {
    for src in [
        format!("{KEY}const o: any = {{ n: 1, get [Symbol.toStringTag]() {{ this.n = 2; return \"x\"; }} }};\nconsole.log(o[k](), o.n);"),
        format!("{KEY}const o: any = {{ n: 1, toString() {{ this.n = 2; return \"x\"; }} }};\nconsole.log(o[k](), o.n);"),
        format!("{KEY}const key = {{ n: 1, toString() {{ this.n = 2; return \"a\"; }} }};\nconst p: any = {{}};\nconsole.log(p[k](key), key.n);"),
    ] {
        let pf = facts(&src);
        let (_, ro) = field_n(&pf);
        assert!(!ro, "{src}\nn stayed readonly: the method's program code did not run\n{}", pf.dump_text());
    }
}

/// A callee that really is unknown (⊤) still escapes its argument.
#[test]
fn object_proto_method_control_unknown_callee() {
    let src = "const u: any = (globalThis as any)[process.argv[2]];\nconst o = { n: 1 };\nu.m(o);\nconsole.log(o.n);";
    let pf = facts(src);
    let (t, ro) = field_n(&pf);
    assert!(
        t.escape.is_some() && !ro,
        "{src}\nescape={:?}\n{}",
        t.escape,
        pf.dump_text()
    );
}
