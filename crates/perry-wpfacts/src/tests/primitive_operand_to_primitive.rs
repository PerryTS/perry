//! P1: an operand of a primitive builtin operation is converted with
//! ToPrimitive (`@@toPrimitive`, else `valueOf` / `toString`); the operation
//! reads no other key of it and never calls a function found under any key.
//! A function value used as such an operand (tsc emitter.ts `generateName`
//! handed on as a name part) is not called back and does not escape.
use super::*;

fn check_site(src: &str, marker: &str, key: &str, proven: bool) -> Result<(), String> {
    let at = src.find(marker).expect("named marker");
    let start = src[..at].rfind('\n').map_or(0, |n| n + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == key && (start..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing {marker}/{key}\n{}", pf.dump_text()));
    }
    for s in sites {
        if !matches!(s.verdict, Verdict::No(_)) != proven {
            return Err(format!("{marker}/{key}: {:?}\n{}", s.verdict, pf.dump_text()));
        }
    }
    Ok(())
}

/// Primitive builtin operations whose operand is only coerced (ToString /
/// ToNumber) or not looked at; `X` is the operand. (`"s".at(X)` runs the
/// string-method path and `fs.existsSync` its runtime declaration: neither
/// reaches this rule; `fs.readFileSync` reads a file URL: not this rule.)
const OPS: &[(&str, &str)] = &[
    ("symbol", "Symbol(X)"),
    ("symbol_for", "Symbol.for(X)"),
    ("from_char_code", "String.fromCharCode(X)"),
    ("from_code_point", "String.fromCodePoint(X)"),
    ("atob", "atob(X)"),
    ("btoa", "btoa(X)"),
    ("path_join", "path.join(\"a\", X)"),
    ("path_basename", "path.basename(X)"),
    ("path_dirname", "path.dirname(X)"),
    ("chdir", "process.chdir(X)"),
];

const HEAD: &str = r#"
import * as path from "path";
import * as fs from "fs";
declare function external(x: any): void;
class Payload { n = 1; }
const payload = new Payload();
"#;

/// The emitter.ts shape: a function value whose body stores into its
/// parameter, used as an operand. Called back with anything, it would store
/// `payload` into an unknown object.
fn function_operand(op: &str) -> String {
    let use_ = op.replace('X', "(generateName as any)");
    format!(
        r#"{HEAD}
function generateName(name: any) {{ name.saved = payload; return "g"; }}
generateName({{}});
const r = {use_};
console.log(payload.n); // WITNESS
"#
    )
}

/// An object operand with a conversion method and an unrelated method that
/// would leak `payload` if it ran.
fn object_operand(op: &str, conv: &str) -> String {
    let use_ = op.replace('X', "(arg as any)");
    format!(
        r#"{HEAD}
const arg = {{ {conv}, unrelated() {{ external(payload); }} }};
const r = {use_};
console.log(payload.n); // WITNESS
"#
    )
}

fn matrix(make: impl Fn(&str) -> String, proven: bool) -> Vec<String> {
    let mut failures = Vec::new();
    for (name, op) in OPS {
        let src = make(op);
        if let Err(error) = check_site(&src, "// WITNESS", "n", proven) {
            failures.push(format!("{name}: {}", error.lines().next().unwrap()));
        }
    }
    failures
}

#[test]
fn function_operand_is_not_called_back() {
    let f = matrix(function_operand, true);
    assert!(f.is_empty(), "function operand escaped: {f:?}");
}

#[test]
fn unrelated_method_of_operand_never_runs() {
    let forms = [
        r#"toString() { return "x"; }"#,
        r#"toString: undefined, valueOf() { return 1; }"#,
        r#"[Symbol.toPrimitive](hint: any) { return 1; }"#,
    ];
    let mut f = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        for e in matrix(|op| object_operand(op, form), true) {
            f.push(format!("form{i} {e}"));
        }
    }
    assert!(f.is_empty(), "unrelated method ran: {f:?}");
}

/// The conversion method itself runs, with the operand as `this`.
#[test]
fn conversion_method_runs_with_operand_as_this() {
    let forms = [
        r#"box: payload, toString() { external(this.box); return "x"; }"#,
        r#"box: payload, toString: undefined, valueOf() { external(this.box); return 1; }"#,
        r#"box: payload, [Symbol.toPrimitive](hint: any) { external(this.box); return 1; }"#,
    ];
    let mut f = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        for e in matrix(|op| object_operand(op, form), false) {
            f.push(format!("form{i} {e}"));
        }
    }
    assert!(f.is_empty(), "conversion method did not run: {f:?}");
}

/// Control: a `@@toPrimitive` from unknown code is called with the operand
/// as `this`: the operand and what it holds escape.
#[test]
fn unknown_to_primitive_stays_top() {
    let f = matrix(
        |op| {
            let use_ = op.replace('X', "(arg as any)");
            format!(
                r#"{HEAD}
const u = new Function(process.argv[2])();
const arg = {{ box: payload, [Symbol.toPrimitive]: u }};
const r = {use_};
console.log(payload.n); // WITNESS
"#
            )
        },
        false,
    );
    assert!(f.is_empty(), "unknown @@toPrimitive did not escape: {f:?}");
}
