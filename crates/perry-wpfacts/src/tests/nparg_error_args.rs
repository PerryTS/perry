//! Rule NPARG: `super(message, options)` into a realm Error-family parent
//! (`Error`, `TypeError`, `RangeError`, `SyntaxError`, `ReferenceError`)
//! acts on its arguments only per the constructor's contract (ECMA-262
//! 20.5.1.1 Error, 20.5.6.1.1 NativeError, 20.5.8.1 InstallErrorCause; the
//! runtime's `js_error_subclass_default_init_with_options` and
//! `js_error_apply_cause_to_object`): `message` through ToString (a program
//! `toString` / `valueOf` / `@@toPrimitive` runs with `this` = the message),
//! `options` through one [[Get]] of `cause` whose result becomes the
//! instance's own `cause`; later arguments are never read. Red on base: every
//! argument was touched as `Protocol::Any`, so a closure message escaped and
//! an object message had its methods called back with anything, and `cause`
//! was never stored (a read of `e.cause` missed the value).
use super::*;

const FAMILY: &[&str] = &[
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
];

fn at<'a>(pf: &'a ProgramFacts, src: &str, marker: &str, key: &str) -> Vec<&'a SiteFacts> {
    let m = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker {marker}"));
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == key && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `{key}` site on the {marker} line");
    s
}

fn show(s: &[&SiteFacts]) -> String {
    format!(
        "{:?}",
        s.iter()
            .map(|s| (&s.receivers, s.top, &s.verdict))
            .collect::<Vec<_>>()
    )
}

/// Every site on the line: no ⊤ receiver, a proven verdict, and (`ran`)
/// at least one receiver the solve found.
fn check(pf: &ProgramFacts, src: &str, marker: &str, key: &str, ran: bool) {
    let s = at(pf, src, marker, key);
    assert!(
        s.iter()
            .all(|s| !s.top && !matches!(s.verdict, Verdict::No(_)) && (!ran || !s.receivers.is_empty())),
        "{marker} `{key}`: {}",
        show(&s)
    );
}

/// The NPARG1 fixture f2/a.ts (r037 node.ts:30 `class DOMException extends
/// Error`): a closure and an object reach `message`.
fn closure_message(parent: &str) -> String {
    format!(
        "class DOMException extends {parent} {{\n\
           constructor(message: any, name = \"Error\") {{ super(message); this.name = name; }}\n\
         }}\n\
         class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
         const a = new V(1), b = new V(2);\n\
         const cb = (p: V): number => p.x; // SITE cb\n\
         function f(m: any): string {{ try {{ throw new DOMException(m, \"N\"); }} catch (e: any) {{ return e.name; }} }}\n\
         console.log(f(\"q\"), f(a), f(cb));\n\
         console.log(a.x);\n\
         console.log(cb(b));\n"
    )
}

/// Red on base: the closure handed as `message` escaped (escaped_beside_top,
/// "native parent constructor argument"), so `p.x` in it had a ⊤ receiver.
/// ToString of a function calls only `Function.prototype.toString` (or a
/// program `toString` the solve sees), never the closure itself.
#[test]
fn a_closure_message_does_not_escape() {
    for parent in FAMILY {
        let src = closure_message(parent);
        let pf = facts(&src);
        check(&pf, &src, "// SITE cb", "x", true);
    }
}

/// Control (f2/b.ts): string messages only, proven before and after.
#[test]
fn string_messages_stay_proven() {
    let src = "class DOMException extends Error {\n\
                 constructor(message: any, name = \"Error\") { super(message); this.name = name; }\n\
               }\n\
               class V { x: number; constructor(x: number) { this.x = x; } }\n\
               const a = new V(1);\n\
               function f(m: string): string { try { throw new DOMException(m, \"N\"); } catch (e: any) { return e.message; } }\n\
               console.log(f(\"q\"), f(\"r\"), a.x); // SITE\n";
    let pf = facts(src);
    check(&pf, src, "// SITE", "x", true);
}

/// A program `toString` on the message still runs (ToPrimitive, `this` =
/// the message object) and only so: `this.t` sees the literal, not
/// anything. Red on base: the Any touch called it back with any receiver.
#[test]
fn a_program_to_string_on_the_message_runs_with_the_message_as_this() {
    for parent in FAMILY {
        let src = format!(
            "class E extends {parent} {{ constructor(m: any) {{ super(m); }} }}\n\
             class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
             const a = new V(1);\n\
             const msg = {{ t: a, toString(): string {{ return String(this.t.x); }} }}; // SITE t\n\
             const e = new E(msg);\n\
             console.log(e.message, a.x);\n"
        );
        let pf = facts(&src);
        check(&pf, &src, "// SITE t", "t", true);
    }
}

/// `options.cause` is read by [[Get]] (a getter runs, `this` = options) and
/// its result is the instance's own `cause`: `e.cause.x` sees `V`. Red on
/// base: `cause` was never stored, so the read missed the value (no
/// receiver), and the getter was called back with anything.
#[test]
fn the_cause_getter_runs_and_the_instance_holds_its_result() {
    for parent in FAMILY {
        let src = format!(
            "class E extends {parent} {{ constructor(m: string, o: any) {{ super(m, o); }} }}\n\
             class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
             const a = new V(1);\n\
             const opts = {{ v: a, get cause(): V {{ return this.v; }} }}; // SITE v\n\
             const e: any = new E(\"m\", opts);\n\
             console.log(e.cause.x); // SITE cause\n"
        );
        let pf = facts(&src);
        check(&pf, &src, "// SITE v", "v", true);
        check(&pf, &src, "// SITE cause", "x", true);
    }
}

/// A data `cause` (`Error(msg, { cause })`) for every family member: the
/// instance's `cause` holds it; an argument past `options` is never read.
#[test]
fn a_data_cause_is_stored_on_every_family_member() {
    for parent in FAMILY {
        let src = format!(
            "class E extends {parent} {{ constructor(m: string, o: any, z: any) {{ super(m, o, z); }} }}\n\
             class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
             const a = new V(1), b = new V(2);\n\
             const z = (p: V): number => p.x; // SITE z\n\
             const e: any = new E(\"m\", {{ cause: a }}, z);\n\
             console.log(e.cause.x, z(b)); // SITE cause\n"
        );
        let pf = facts(&src);
        check(&pf, &src, "// SITE cause", "x", true);
        check(&pf, &src, "// SITE z", "x", true);
    }
}

/// The generic builtin constructor (`new C(m, o)` with `C` the realm's
/// `Error` / NativeError / `AggregateError` reached as a value; the
/// `new Error(..)` syntax is the exact `Expr::*ErrorNew` arms) follows the
/// same contract as the subclass `super(...)`. Red on base: every operand was
/// touched as `Protocol::Any`, so a closure handed as `message` escaped.
/// `form`: `("Error", "")` is the literal syntax (keep), `("C", "const C: any
/// = Error;\n")` the value.
fn forms(ctor: &str) -> [(String, String); 2] {
    [
        (ctor.to_string(), String::new()),
        ("C".to_string(), format!("const C: any = {ctor};\n")),
    ]
}

#[test]
fn a_closure_message_of_a_plain_constructor_does_not_escape() {
    for ctor in FAMILY {
        for (name, pre) in forms(ctor) {
            let src = format!(
                "{pre}class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
                 const a = new V(1), b = new V(2);\n\
                 const cb = (p: V): number => p.x; // SITE cb\n\
                 function f(m: any): string {{ try {{ throw new {name}(m); }} catch (e: any) {{ return e.name; }} }}\n\
                 console.log(f(\"q\"), f(cb));\n\
                 console.log(a.x);\n\
                 console.log(cb(b));\n"
            );
            let pf = facts(&src);
            check(&pf, &src, "// SITE cb", "x", true);
        }
    }
}

/// `new C(m, { get cause() })`: the getter runs with `this` = options and
/// the instance's own `cause` holds its result; a third operand is never read.
#[test]
fn the_cause_getter_of_a_plain_constructor_runs_and_the_instance_holds_its_result() {
    for ctor in FAMILY {
        for (name, pre) in forms(ctor) {
            // The literal `new Error(..)` syntax lowers only operands 0 and 1.
            let extra = if name == "C" { ", z" } else { "" };
            let src = format!(
                "{pre}class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
                 const a = new V(1), b = new V(2);\n\
                 const z = (p: V): number => p.x; // SITE z\n\
                 const opts = {{ v: a, get cause(): V {{ return this.v; }} }}; // SITE v\n\
                 const e: any = new {name}(\"m\", opts{extra});\n\
                 console.log(e.cause.x, z(b)); // SITE cause\n"
            );
            let pf = facts(&src);
            check(&pf, &src, "// SITE v", "v", true);
            check(&pf, &src, "// SITE cause", "x", true);
            check(&pf, &src, "// SITE z", "x", true);
        }
    }
}

/// A program `toString` on the message of a plain constructor runs with the
/// message as `this`, and only so.
#[test]
fn a_program_to_string_on_a_plain_constructor_message_runs() {
    for (name, pre) in forms("Error") {
        let src = format!(
            "{pre}class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
             const a = new V(1);\n\
             const msg = {{ t: a, toString(): string {{ return String(this.t.x); }} }}; // SITE t\n\
             const e = new {name}(msg as any);\n\
             console.log(e.message, a.x);\n"
        );
        let pf = facts(&src);
        check(&pf, &src, "// SITE t", "t", true);
    }
}

/// AggregateError (runtime `js_aggregateerror_new_full`): operand 0 is
/// iterated into `errors`, operand 1 is the message (ToString), operand 2 is
/// the options whose `cause` is [[Get]]; later operands are never read.
#[test]
fn aggregate_error_operands_follow_the_runtime_contract() {
    for (name, pre) in forms("AggregateError") {
        let src = format!(
            "{pre}class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
             const a = new V(1), b = new V(2);\n\
             const z = (p: V): number => p.x; // SITE z\n\
             const opts = {{ v: a, get cause(): V {{ return this.v; }} }}; // SITE v\n\
             const e: any = new {name}([a], \"m\", opts, z);\n\
             console.log(e.cause.x, z(b)); // SITE cause\n\
             console.log(e.errors[0].x); // SITE errors\n"
        );
        let pf = facts(&src);
        check(&pf, &src, "// SITE v", "v", true);
        check(&pf, &src, "// SITE cause", "x", true);
        check(&pf, &src, "// SITE errors", "x", true);
        check(&pf, &src, "// SITE z", "x", true);
    }
}

/// AggregateError message is operand 1: a closure there does not escape.
#[test]
fn an_aggregate_error_closure_message_does_not_escape() {
    for (name, pre) in forms("AggregateError") {
        let src = format!(
            "{pre}class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
             const a = new V(1), b = new V(2);\n\
             const cb = (p: V): number => p.x; // SITE cb\n\
             function f(m: any): string {{ try {{ throw new {name}([a], m); }} catch (e: any) {{ return e.name; }} }}\n\
             console.log(f(\"q\"), f(cb), a.x, cb(b));\n"
        );
        let pf = facts(&src);
        check(&pf, &src, "// SITE cb", "x", true);
    }
}
