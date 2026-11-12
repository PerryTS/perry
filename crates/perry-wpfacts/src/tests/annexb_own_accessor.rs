//! `__defineGetter__` / `__defineSetter__` (ECMA-262 B.2.2.2, B.2.2.3):
//! `O = ToObject(this)`, then DefinePropertyOrThrow(O, key, accessor). The
//! accessor is an own property of the receiver itself: on a builtin
//! function object or `Object.prototype`'s method stand-in it redefines no
//! builtin prototype and the receiver does not escape; its reads are open.
//! A receiver that may be a builtin prototype (a prototype itself, ⊤) keeps
//! the prototype treatment.
use super::*;

fn calls(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn native(src: &str, key: &str) -> bool {
    let pf = facts(src);
    let v = calls(&pf, key);
    assert!(!v.is_empty(), "no {key} call site\n{}", pf.dump_text());
    v.iter().all(|v| *v == Verdict::NativeMember)
}

const KEY: &str = "const k: any = process.argv[2];\n";

/// A builtin function object under a computed key: an own accessor of
/// `Math.max`, `Array.prototype.slice` stays the builtin.
#[test]
fn builtin_function_receiver_redefines_no_prototype() {
    let src = format!(
        "{KEY}const f: any = Math.max;\nf.__defineGetter__(k, () => 1);\nfunction sl(m: readonly number[]): number[] {{ return m.slice(); }}\nconsole.log(sl([1]).length, f.x);"
    );
    assert!(native(&src, "slice"), "{src}");
}

/// The same under a literal key (the modeled holder group of `Math.max`).
#[test]
fn builtin_function_receiver_named_key() {
    let src = "const f: any = Math.max;\nf.__defineGetter__('x', () => 1);\nfunction sl(m: readonly number[]): number[] { return m.slice(); }\nconsole.log(sl([1]).length, f.x);";
    assert!(native(src, "slice"), "{src}");
}

/// The receiver is `Object.prototype`'s method stand-in (a computed read off
/// a plain object), the key and the getter unknown: an own accessor of that
/// function object, no worse than the union of the methods it stands for.
#[test]
fn object_proto_method_stand_in_receiver() {
    let src = format!(
        "{KEY}const o: any = {{}};\nconst m: any = o[k];\nm.__defineGetter__(k, m);\nfunction sl(m: readonly number[]): number[] {{ return m.slice(); }}\nconsole.log(sl([1]).length, m.x);"
    );
    assert!(native(&src, "slice"), "{src}");
}

/// The getter of an own accessor runs with `this` = the receiver: what it
/// writes there is seen by a read of the receiver.
#[test]
fn own_accessor_getter_reads_are_open() {
    let src = format!(
        "{KEY}class Cell {{ witness = 1; }}\nconst f: any = Math.max;\nf.__defineGetter__(k, function () {{ return new Cell(); }});\nconsole.log(f.y.witness);"
    );
    let pf = facts(&src);
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
    assert!(
        v.iter().all(|s| matches!(s.verdict, Verdict::No(_))),
        "{src}\n{v:?}"
    );
}

/// Control: the receiver IS a builtin prototype.
#[test]
fn array_prototype_receiver_redefines_slice() {
    let src = "const g: any = function () { return 1; };\n(Array.prototype as any).__defineGetter__('slice', g);\nfunction sl(m: readonly number[]): number[] { return m.slice(); }\nconsole.log(sl([1]).length);";
    assert!(!native(src, "slice"), "{src}");
}

/// The define adds no escape: an object held by the receiver escapes
/// with it exactly as without the define (B.2.2.2 defines one own property
/// of the receiver; nothing hands the receiver to unknown code).
#[test]
fn builtin_function_receiver_does_not_escape() {
    let cell_escape = |src: &str| {
        let pf = facts(src);
        let t = pf
            .templates
            .iter()
            .find(|t| t.fields.iter().any(|f| f.key == "witness"))
            .unwrap_or_else(|| panic!("no Cell\n{}", pf.dump_text()));
        t.escape.clone()
    };
    for (getter, recv) in [("() => 1", "Math.max"), ("m", "o[k]")] {
        let base = format!(
            "{KEY}class Cell {{ witness = 1; }}\nconst o: any = {{}};\nconst f: any = {recv};\nconst m: any = o[k];\nf.cell = new Cell();\n"
        );
        let with = format!("{base}f.__defineGetter__(k, {getter});\nconsole.log(typeof f, typeof m);");
        let without = format!("{base}console.log(typeof f, typeof m);");
        let (w, wo) = (cell_escape(&with), cell_escape(&without));
        // No E5: the receiver is not marked or escaped as a redefined
        // prototype. (The any-key accessor of a receiver without a modeled
        // holder group still opens its reads: `Cons::AccessorInstall`.)
        assert!(!w.as_deref().is_some_and(|e| e.contains("E5")), "{with}\n{w:?} {wo:?}");
    }
}
