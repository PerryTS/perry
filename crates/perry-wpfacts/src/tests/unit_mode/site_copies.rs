//! A read site two units walk: `letter` is its own unit's body and, inlined
//! into `App.render` (an inlined body is a copy of its own), `render`'s
//! unit's too. `letter`'s own unit gets no arguments (its one caller runs
//! the copy): its record is a dead site. One run unites the records of a
//! site walked twice (`derive::site_facts`); a dead record adds nothing to
//! that union, so the copy's unit decides the site, as the whole program.

use super::*;

const P: &str = r#"export interface Piece { type: string; color: string; }
export function piece(i: number): Piece | null {
    return i > 100 ? null : { type: i % 2 === 0 ? "p" : "n", color: i % 3 === 0 ? "w" : "b" };
}"#;

const A: &str = r#"import { piece } from "./p";
import type { Piece } from "./p";
function letter(pc: Piece | null, upper: boolean): string {
    if (pc === null) return ".";
    let l = "?";
    if (pc.type === "p") l = "p";
    else if (pc.type === "n") l = "n";
    return pc.color === "w" || upper ? l.toUpperCase() : l;
}
export class App {
    upper = false;
    render(): string {
        let s = "";
        for (let i = 0; i < 8; i++) {
            let l = letter(piece(i), this.upper);
            s += l;
        }
        return s;
    }
}"#;

const MAIN: &str = r#"import { App } from "./a";
console.log(new App().render());"#;

/// The modules as the compiler hands them over: functions inlined
/// (`perry_transform::inline_functions`), as the driver does before facts.
fn inlined_hirs(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
    let mut hirs = hirs_of(mods);
    for (_, m) in &mut hirs {
        perry_transform::inline_functions(
            m,
            &Default::default(),
            &Default::default(),
            &Default::default(),
            &Default::default(),
        );
        perry_hir::monomorphize_module(m);
    }
    hirs
}

fn both(mods: &[(&str, &str)]) -> (ProgramFacts, ProgramFacts) {
    let hirs = inlined_hirs(mods);
    let list: Vec<(String, &perry_hir::Module)> = hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    (
        crate::compute_units(&list, &FactsConfig::default()),
        crate::compute(&list, &FactsConfig::default()),
    )
}

fn a_sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && pf.module_paths[s.module as usize] == "a.ts")
        .collect()
}

#[test]
fn a_dead_record_of_a_site_another_unit_walks_adds_nothing() {
    let mods = [("p", P), ("a", A), ("main", MAIN)];
    let (u, w) = both(&mods);
    let stronger: Vec<_> = proven_set(&u).difference(&proven_set(&w)).cloned().collect();
    assert!(stronger.is_empty(), "per unit stronger than whole: {stronger:?}\n{}", u.dump_text());
    for k in ["type", "color"] {
        let (us, ws) = (a_sites(&u, k), a_sites(&w, k));
        assert!(!ws.is_empty() && ws.iter().all(|s| matches!(s.verdict, Verdict::T0Mono)), "whole {k}\n{}", w.dump_text());
        // One fact per site, as whole: the dead record is gone.
        assert_eq!(us.len(), ws.len(), "{k}\n{}", u.dump_text());
        assert!(us.iter().all(|s| matches!(s.verdict, Verdict::T0Mono)), "unit {k}\n{}", u.dump_text());
    }
}

/// The copy's unit refusing the site (an unknown value reaches the
/// parameter) decides it: the dead record beside it hides nothing.
#[test]
fn a_dead_record_beside_a_refusing_one_stays_refused() {
    let a = A.replace(
        "let l = letter(piece(i), this.upper);",
        "let l = letter((i > 3 ? piece(i) : JSON.parse(\"{}\")) as any, this.upper);",
    );
    assert_ne!(a, A);
    let mods = [("p", P), ("a", a.as_str()), ("main", MAIN)];
    let (u, w) = both(&mods);
    let stronger: Vec<_> = proven_set(&u).difference(&proven_set(&w)).cloned().collect();
    assert!(stronger.is_empty(), "per unit stronger than whole: {stronger:?}\n{}", u.dump_text());
    let us = a_sites(&u, "type");
    assert_eq!(us.len(), a_sites(&w, "type").len(), "{}", u.dump_text());
    assert!(!us.is_empty(), "{}", u.dump_text());
    for s in &us {
        assert!(!matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set), "{}", u.dump_text());
        assert!(!matches!(&s.verdict, Verdict::No(r) if r.starts_with("dead site")), "{}", u.dump_text());
    }
}
