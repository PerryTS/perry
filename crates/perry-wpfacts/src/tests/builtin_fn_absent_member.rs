//! A built-in function object (an Object.prototype method such as
//! `Object.prototype.toString`, or the one a computed read of Object.prototype
//! hands out) owns `length` and `name` (ECMA-262 10.2.9 SetFunctionName,
//! 10.2.10 SetFunctionLength) and inherits from Function.prototype (20.2.3)
//! and Object.prototype (20.1.3). A String name none of them holds reads
//! undefined (10.1.8.1 OrdinaryGet) and its call throws a TypeError
//! (13.3.6.2 EvaluateCall, 7.3.14 Call step 2): neither the receiver nor an
//! argument is handed on. A name the program or unknown code writes on
//! Function.prototype runs what was written. (GLTFLoader.js getDependencies:
//! `(this.json[type + "s"] || []).map(function (def, index) {...})`.)

use super::*;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty()
        && s.iter()
            .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn src(setup: &str, recv: &str) -> String {
    format!(
        "class A {{ a = 1; }}
         {setup}
         const a = new A();
         const d: any = {recv};
         d.held = a;
         try {{
             console.log(d.map(function (x: any, i: number) {{ a.a = x; return i; }}));
         }} catch (e) {{ console.log(\"TypeError\"); }}
         console.log(a.a);"
    )
}

/// `Object.prototype.toString.map(cb)` and the computed-read form
/// `({} as any)[k] || []` flowing into `.map(cb)`: `map` is absent on the
/// function chain, the call throws, `cb` runs nowhere.
#[test]
fn an_absent_member_call_on_a_builtin_function_hands_nothing_on() {
    for recv in [
        "Object.prototype.toString as any",
        "Object.prototype.valueOf as any",
        "({} as any)[process.argv[2]] || []",
        "({} as any)[process.argv[2] + \"s\"] || []",
    ] {
        let s = src("", recv);
        let pf = facts(&s);
        assert!(proven(&pf, "a"), "{s}\n{}", pf.dump_text());
    }
}

/// Control: unknown code writes `Function.prototype.map`; the call runs it
/// with the function and `cb`.
#[test]
fn a_written_function_prototype_member_runs_what_was_written() {
    for recv in [
        "Object.prototype.toString as any",
        "({} as any)[process.argv[2] + \"s\"] || []",
    ] {
        let s = src(
            "(Function.prototype as any).map = new Function(process.argv[2])();",
            recv,
        );
        let pf = facts(&s);
        assert!(!proven(&pf, "a"), "{s}\n{}", pf.dump_text());
    }
}
