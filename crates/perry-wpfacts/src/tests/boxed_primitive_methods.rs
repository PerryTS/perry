//! A primitive wrapper object (`new Number(x)`, `new String(x)`,
//! `new Boolean(x)`) owns no methods: a method call on it reads the name on
//! Number / String / Boolean.prototype and Object.prototype (ECMA-262
//! 10.1.8.1 OrdinaryGet). A name none of them holds reads undefined and the
//! call throws (13.3.6.2 EvaluateCall, 7.3.14 Call) without handing its
//! arguments to anything; a name a program writes onto that chain runs
//! what was written. (three.core.js `Material.setValues`:
//! `currentValue.set(newValue)` on a receiver that also holds a wrapper.)

use super::*;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty()
        && s.iter()
            .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn src(setup: &str, wrap: &str) -> String {
    format!(
        "class A {{ a = 1; }}
         class V {{ v: any = 0; set(x: any) {{ this.v = x; return this; }} }}
         {setup}
         const a = new A();
         const r: any = process.argv.length > 9 ? {wrap} : new V();
         try {{ r.set(a); }} catch (e) {{ console.log('TypeError'); }}
         console.log(a.a);"
    )
}

/// `set` is on none of the wrapper chains: the argument is handed to
/// nothing on the wrapper side.
#[test]
fn an_absent_method_of_a_primitive_wrapper_takes_no_argument() {
    for wrap in ["new Number(3)", "new String('s')", "new Boolean(true)"] {
        let s = src("", wrap);
        let pf = facts(&s);
        assert!(proven(&pf, "a"), "{s}\n{}", pf.dump_text());
    }
}

/// Control: unknown code writes `Number.prototype.set`; the wrapper's call
/// runs it with the argument.
#[test]
fn a_method_unknown_code_writes_on_the_wrapper_prototype_takes_the_argument() {
    let s = src(
        "(Number.prototype as any).set = new Function(process.argv[2])();",
        "new Number(3)",
    );
    let pf = facts(&s);
    assert!(!proven(&pf, "a"), "{s}\n{}", pf.dump_text());
}

/// A wrapper's own realm methods `valueOf` / `toString` read only the
/// wrapped primitive (thisNumberValue / thisStringValue /
/// thisBooleanValue, ECMA-262 21.1.3.7, 22.1.3.35, 20.3.3.3; 21.1.3.6
/// coerces only `radix`): neither the wrapper nor what it holds is handed
/// to anything.
#[test]
fn a_wrapper_value_of_hands_neither_the_wrapper_nor_its_fields() {
    for wrap in ["new Number(3)", "new String('s')", "new Boolean(true)"] {
        let s = format!(
            "class A {{ a = 1; }}
             const a = new A();
             const w: any = {wrap};
             w.held = a;
             console.log(w.valueOf(), w.toString());
             console.log(a.a);"
        );
        let pf = facts(&s);
        assert!(proven(&pf, "a"), "{s}\n{}", pf.dump_text());
    }
}
