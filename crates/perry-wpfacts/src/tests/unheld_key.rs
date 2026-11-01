//! A key the defining unit's layout never holds (`units::unheld`): its
//! own method calls it on the instance (`this.status()` in `play`), so the
//! layout's facts name `status` as a field, never written, whose kind is
//! the `undefined` of a key not present. Another unit's stand-in must not
//! seed that `undefined` into an own field: the key is the chain's, and a
//! call of it there finds the method, as in the defining unit.

use super::*;

fn modules(mods: &[(&str, &str)], per_unit: bool) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let cfg = FactsConfig::default();
    match per_unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

/// Every `.status` call site of module `m.ts`, proven with one target.
fn status_calls_proven(pf: &ProgramFacts) -> bool {
    let s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "status" && s.module == 1)
        .collect();
    assert_eq!(s.len(), 2, "two .status calls in m.ts");
    s.iter().all(|s| matches!(s.verdict, Verdict::T0Mono))
}

const ENG: &str = r#"
export class Chess {
  private board: number[] = [1, 2];
  play(m: string): string | null {
    if (this.status() !== "playing") return null;
    this.board.push(m.length);
    return m;
  }
  status(): string { return this.board.length < 9 ? "playing" : "draw"; }
}
"#;

// Node: prints `playing` twice.
const MAIN: &str = r#"
import { Chess } from "./a";
const c = new Chess();
if (c.play("e2e4") === null) throw new Error("e2e4");
console.log(c.status());
console.log(new Chess().status());
"#;

#[test]
fn a_method_its_class_calls_on_itself_is_found_on_the_chain_per_unit() {
    let w = modules(&[("a", ENG), ("m", MAIN)], false);
    assert!(status_calls_proven(&w), "whole program");
    let u = modules(&[("a", ENG), ("m", MAIN)], true);
    assert!(status_calls_proven(&u), "per unit as whole");
}

#[test]
fn an_own_field_another_unit_stores_still_shadows_the_method() {
    // Node: prints `playing` and `x`: `status` is an own field of `c`.
    let main = r#"
import { Chess } from "./a";
const c: any = new Chess();
if (c.play("e2e4") === null) throw new Error("e2e4");
console.log(c.status());
c.status = () => "x";
console.log(c.status());
"#;
    let u = modules(&[("a", ENG), ("m", main)], true);
    let s: Vec<_> = u
        .sites
        .iter()
        .filter(|s| s.key == "status" && s.module == 1 && s.is_call)
        .collect();
    assert!(!s.is_empty());
    assert!(
        s.iter().all(|s| !matches!(s.verdict, Verdict::T0Mono)),
        "an own field stored after birth is not the method: {:?}",
        s.iter().map(|s| &s.verdict).collect::<Vec<_>>()
    );
}
