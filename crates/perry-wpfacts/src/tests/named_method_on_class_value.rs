//! A named `next`/`return`/`throw` read on a class or function value that is
//! no generator step is an ordinary [[Get]]: the value's own statics, then
//! `Function.prototype`, then `Object.prototype`. None of them defines these
//! keys unless the program put one there (an own static shadows the builtin
//! members; a key written onto the chain through unknown code is the
//! written-method arrival), so the call runs the program's own method or
//! throws a TypeError (the region every `catch` receives); no unknown code
//! runs and the receiver does not escape. A computed key on a function value
//! is a runtime property: unknown.
//! Controls: `next` defined on the class with a value from unknown code, and
//! a computed-key store of an unknown value onto the class.
use super::*;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty()
        && s.iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn not_escaped(pf: &ProgramFacts, class: &str) {
    let t = tmpl(pf, class);
    assert!(
        t.escape.is_none(),
        "{class}: escape {:?}\n{}",
        t.escape,
        pf.dump_text()
    );
}

fn escaped(pf: &ProgramFacts, class: &str) {
    let t = tmpl(pf, class);
    assert!(
        t.escape.is_some(),
        "{class}: not escaped\n{}",
        pf.dump_text()
    );
}

const STEP: &str = "class Links { x = 1 }\nfunction f(v: any) { for (const t of v) { console.log(t); } }\nf([1]);\n";

/// The seed: the for-of step on a class constructor reads `next` on it.
#[test]
fn named_next_on_a_class_value_in_for_of_is_an_ordinary_lookup() {
    let pf = facts(&format!(
        "{STEP}f(Links as any);\nconst l = new Links();\nconsole.log(l.x);\n"
    ));
    not_escaped(&pf, "Links");
    assert!(proven(&pf, "x"), "{}", pf.dump_text());
}

/// An explicit `C.next()` / `C.return()` / `C.throw()` on a class without
/// such statics throws a TypeError: nothing escapes.
#[test]
fn named_next_return_throw_called_on_a_class_value_throw_type_error() {
    let pf = facts(&format!(
        "{STEP}try {{ (Links as any).next(); }} catch (e) {{ console.log(e); }}\n\
         try {{ (Links as any).return(1); }} catch (e) {{ console.log(e); }}\n\
         try {{ (Links as any).throw(2); }} catch (e) {{ console.log(e); }}\n\
         const l = new Links();\nconsole.log(l.x);\n"
    ));
    not_escaped(&pf, "Links");
    assert!(proven(&pf, "x"), "{}", pf.dump_text());
}

/// A class that defines `static next()`: that method is the callee, exactly
/// (its result is the program's own object).
#[test]
fn a_static_next_on_a_class_is_the_callee() {
    let pf = facts(
        "class Got { y = 2 }\n\
         class Links { x = 1; static next() { return new Got(); } }\n\
         function f(v: any) { return v.next(); }\n\
         const g = f(Links);\nconsole.log(g.y);\n\
         const l = new Links();\nconsole.log(l.x);\n",
    );
    not_escaped(&pf, "Links");
    not_escaped(&pf, "Got");
    assert!(proven(&pf, "y"), "{}", pf.dump_text());
}

/// Control: `next` defined on the class with a value from unknown code: the
/// named lookup finds that value, the call runs unknown code.
#[test]
fn control_next_defined_from_unknown_code_stays_top() {
    let pf = facts(&format!(
        "{STEP}const v = new Function(process.argv[2])();\n\
         Object.defineProperty(Links, \"next\", {{ value: v }});\n\
         f(Links as any);\nconst l = new Links();\nconsole.log(l.x);\n"
    ));
    escaped(&pf, "Links");
}

/// Control: a computed-key store of an unknown value onto the class.
#[test]
fn control_computed_store_of_unknown_onto_the_class_stays_top() {
    let pf = facts(&format!(
        "{STEP}const external = new Function(process.argv[2])();\n\
         (Links as any)[process.argv[2]] = external;\n\
         f(Links as any);\nconst l = new Links();\nconsole.log(l.x);\n"
    ));
    escaped(&pf, "Links");
}
