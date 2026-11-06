//! `concat` copies the elements of its array operands (ECMA-262 23.1.3.2:
//! `Get(E, ToString(k))` for each index),
//! never the methods an any-key read of an array could name. An element of
//! the result is therefore no opaque builtin function, and calling it does
//! not hand the receiver to unseen code.
use super::*;

fn payload_proven(src: &str) -> Result<bool, String> {
    let at = src.find("// PAYLOAD_N").expect("named site");
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "n" && (line..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing PAYLOAD_N\n{}", pf.dump_text()));
    }
    Ok(sites.iter().all(|s| !matches!(s.verdict, Verdict::No(_))))
}

fn program(make: &str) -> String {
    format!(
        r#"
declare function external(x: unknown): any;
class Payload {{ n = 1; bump() {{ this.n += 1; return this; }} }}
const payload = new Payload();
const targets: any[] = {make};
for (let i = 0; i < targets.length; i++) targets[i]();
console.log(payload.n); // PAYLOAD_N
"#
    )
}

#[test]
fn concat_results_hold_only_the_operands_elements() {
    let mut failures = Vec::new();
    for make in [
        "[() => payload.bump()].concat([() => payload])",
        "[() => payload.bump()].concat([() => payload], [() => 0])",
    ] {
        match payload_proven(&program(make)) {
            Ok(true) => {}
            Ok(false) => failures.push(make.to_string()),
            Err(e) => failures.push(format!("{make}: {}", e.lines().next().unwrap())),
        }
    }
    assert!(failures.is_empty(), "unproven: {failures:?}");
}

/// Controls: an unknown function among the copied elements and a spreadable
/// plain object (no array: a computed read) keep the receiver handed to
/// unseen code. (A hole reads `undefined`: numeric keys cannot be added to
/// `Array.prototype` or `Object.prototype`, the runtime's lock.)
#[test]
fn concat_of_unknown_or_inherited_elements_stays_unproven() {
    let mut failures = Vec::new();
    for (pre, make) in [
        ("", "[() => payload.bump()].concat([external(0)])"),
        (
            "",
            "[() => payload.bump()].concat({ length: 1, 0: external(3), [Symbol.isConcatSpreadable]: true } as any)",
        ),
    ] {
        let src = program(make).replacen(
            "const targets",
            &format!("{pre}\nconst targets"),
            1,
        );
        match payload_proven(&src) {
            Ok(false) => {}
            Ok(true) => failures.push(make.to_string()),
            Err(e) => failures.push(format!("{make}: {}", e.lines().next().unwrap())),
        }
    }
    assert!(failures.is_empty(), "proven although the callee may be unknown: {failures:?}");
}

/// Control: an array operand whose `Symbol.isConcatSpreadable` is false (set
/// by the program or under a key unknown code chose) is appended whole, so
/// the result's element is the operand itself: `r[0][0]()` runs the closure
/// that hands the payload to unseen code. Dropping the whole-operand edge
/// for the element read would prove PAYLOAD_N wrongly.
#[test]
fn concat_appends_a_non_spreadable_operand_whole() {
    let mut failures = Vec::new();
    for flag in [
        "(inner as any)[Symbol.isConcatSpreadable] = false;",
        "(inner as any)[external(4)] = external(5);",
    ] {
        let src = format!(
            r#"
declare function external(x: unknown): any;
class Payload {{ n = 1; }}
const payload = new Payload();
const inner: any[] = [() => external(payload)];
{flag}
const r: any[] = ([] as any[]).concat(inner);
r[0][0]();
console.log(payload.n); // PAYLOAD_N
"#
        );
        match payload_proven(&src) {
            Ok(false) => {}
            Ok(true) => failures.push(flag.to_string()),
            Err(e) => failures.push(format!("{flag}: {}", e.lines().next().unwrap())),
        }
    }
    assert!(failures.is_empty(), "whole-appended operand lost: {failures:?}");
}
