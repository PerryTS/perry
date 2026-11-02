//! A builtin protocol a callee runs on its parameter (ToPrimitive,
//! iteration …) is published with its protocol: the caller's unit runs
//! that protocol on what it passes, with that protocol's keys
//! (`Protocol::keys`), not a call of every key it holds. GFX: TypeScript's
//! `diagnosticToString(d)` converts `d` on the array arm only
//! (`isArray(d) ? … d.slice(1) …`); as a call of any key, each
//! `Diagnostics.X` record a unit passed was the receiver of its field
//! values, an escape that broke the `diag()` layout in every unit.

use super::*;

const D: &str = r#"function diag(code: number, category: number, key: string, message: string, reportsUnnecessary?: {}, elided?: boolean, deprecated?: {}) {
    return { code, category, key, message, reportsUnnecessary, elidedInCompatabilityPyramid: elided, reportsDeprecated: deprecated };
}
export const Diagnostics = { A: diag(1, 1, "A_1", "a"), B: diag(2, 1, "B_2", "b") };"#;

const U: &str = r#"export function getMsg(m: any) { return m.message; }
export function toStr(d: any): string { return BODY; }
export function action(name: string, d: any) { return { name, description: toStr(d) }; }"#;

const T: &str = r#"import { Diagnostics } from "./d";
import { action } from "./u";
const codes = [Diagnostics.A.code];
console.log(codes[0], action("x", Diagnostics.B).description, Diagnostics.A.message, Diagnostics.B.key);"#;

/// `t.ts` reads of the records' fields, per unit and whole: all proven,
/// per unit no stronger than whole, no layout broken.
fn reads_proven(body: &str) {
    let u_src = U.replace("BODY", body);
    let mods = [("d", D), ("u", u_src.as_str()), ("t", T)];
    let (u, w) = (units_of(&mods), whole_of(&mods));
    let stronger: Vec<_> = proven_set(&u)
        .difference(&proven_set(&w))
        .cloned()
        .collect();
    assert!(
        stronger.is_empty(),
        "per unit stronger than whole: {stronger:?}"
    );
    for pf in [&w, &u] {
        let sites: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| {
                pf.module_paths[s.module as usize] == "t.ts"
                    && ["code", "message", "key"].contains(&s.key.as_str())
            })
            .collect();
        assert_eq!(sites.len(), 3, "{}", pf.dump_text());
        for s in sites {
            assert!(
                matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set),
                "{body}: {}@{} {:?} broken {:?}\n{}",
                s.key,
                s.byte_offset,
                s.verdict,
                pf.broken,
                pf.dump_text()
            );
        }
        assert!(pf.broken.is_empty(), "{body}: broken {:?}", pf.broken);
    }
}

/// ToPrimitive of what the parameter holds under a computed key.
#[test]
fn a_conversion_of_an_element_runs_to_primitive_on_the_argument() {
    reads_proven("Array.isArray(d) ? String(d[0]) : getMsg(d)");
}

/// ToPrimitive of the result of a method the parameter's caller runs.
#[test]
fn a_conversion_of_a_method_result_runs_to_primitive_on_the_argument() {
    reads_proven("Array.isArray(d) ? String(d.slice(1)) : getMsg(d)");
}

/// TypeScript's `diagnosticToString` shape.
#[test]
fn diagnostic_to_string() {
    reads_proven("Array.isArray(d) ? getMsg(d[0]) + d.slice(1).join(\",\") : getMsg(d)");
}

/// A conversion of the parameter itself.
#[test]
fn a_conversion_of_the_parameter_runs_to_primitive_on_the_argument() {
    reads_proven("typeof d === \"string\" ? `${d}` : getMsg(d)");
}
