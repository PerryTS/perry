//! A getter of another unit's class runs where a unit reads it. The whole
//! program runs the getter on every read under its key, including a read
//! that lets the path escape (a callee leaking what its parameter reaches
//! under the key), so what it returns leaves with the read. Per unit the
//! reader's stand-in had no member under the key: a read found `undefined`
//! only, and the getter's owner published its result as a value the next
//! unit reads through its owner's contract, which no reader did. Per unit
//! was stronger than whole. Now the class contract names its getters
//! (`ClassContract::getters`): the reader runs the owner's getter, which
//! returns an unknown value, and the owner lets what its getters return
//! leave beyond any contract (DECISIONS 203/212). Module a defines the
//! functions (their parameters are bare markers), module m calls them;
//! every top-level statement of m is its own unit.

use super::*;

fn hirs(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
    mods.iter()
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
        .collect()
}

fn run(m: &str, units: bool) -> ProgramFacts {
    let mods = [("a", A), ("m", &format!("{CLASSES}{m}")[..])];
    let hirs = hirs(&mods);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    match units {
        true => crate::compute_units(&list, &FactsConfig::default()),
        false => crate::compute(&list, &FactsConfig::default()),
    }
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!s.is_empty(), "no site .{key}");
    s.iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// Module m's classes: `Leaky::get2` leaks its receiver.
const CLASSES: &str = r#"
function leak(o: any): void { (globalThis as any).g = o; }
class Leaky {
  k = 5;
  lm = 7;
  get2(): number { leak(this); return this.k; }
}
class Plain {
  k = 5;
  lm = 7;
}
"#;

/// Module a: `escDeep` lets what its parameter reaches under `host`
/// escape; `bindDeep` calls a method on it (the call escapes its receiver).
const A: &str = r#"
export function escDeep(s: any): void { (globalThis as any).e = s.host; }
export function bindDeep(s: any): any { const host = s.host; return host.get2.bind(host); }
"#;

/// `key` is proven in neither mode: the value leaves in the whole program,
/// and per unit is never stronger.
fn leaves(m: &str, key: &str) {
    assert!(!proven(&run(m, false), key), "whole program .{key}");
    let u = run(m, true);
    assert!(
        !proven(&u, key),
        "per unit .{key} never stronger than whole\n{}",
        u.dump_text()
    );
}

// Node (a.ts + m.ts): prints `9`; `globalThis.e` is pl.
#[test]
fn a_getter_on_a_path_the_callee_lets_escape_leaves() {
    leaves(
        r#"
import { escDeep } from "./a";
const pl = new Plain();
class Getter { get host(): Plain { return pl; } }
escDeep(new Getter());
pl.lm = 9;
console.log(pl.lm);
"#,
        "lm",
    );
}

// Node: prints `5 7`; `globalThis.g` is lk. UEV10's negative (d): the
// receiver of the escaping call is what the getter returns.
#[test]
fn a_getter_on_the_path_runs_in_the_readers_model() {
    leaves(
        r#"
import { bindDeep } from "./a";
const lk = new Leaky();
class Getter { get host(): Leaky { return lk; } }
const g = bindDeep(new Getter());
console.log(g(), lk.lm);
"#,
        "lm",
    );
}

// Node: prints `7`; `globalThis.z` is pl. Getter's unit is not the reading
// statement's.
#[test]
fn a_getter_read_by_another_unit_leaves_with_what_it_returns() {
    leaves(
        r#"
const pl = new Plain();
class Getter { get host(): Plain { return pl; } }
const v: any = new Getter().host;
(globalThis as any).z = v;
console.log(pl.lm);
"#,
        "lm",
    );
}

// Node: prints `7`. An own accessor of a literal on the path (it was
// already left beyond any contract): a control beside the class getters.
#[test]
fn an_own_getter_on_the_path_leaves() {
    leaves(
        r#"
import { escDeep } from "./a";
const pl = new Plain();
escDeep({ get host() { return pl; } });
console.log(pl.lm);
"#,
        "lm",
    );
}

// Node: prints `7`. What a field holds per unit covers what it holds in
// the whole program: the field stores what another unit's getter returns,
// which the reader read as `undefined` only (kind num|undef, no object).
#[test]
fn a_getter_read_by_another_unit_holds_what_it_returns() {
    let m = r#"
const pl = new Plain();
class Getter { get host(): Plain { return pl; } }
class Box { f: any = 0; }
const b = new Box();
b.f = new Getter().host;
console.log(b.f.lm);
"#;
    let w = run(m, false);
    let wk = &field(tmpl(&w, "Box"), "f").kind;
    assert!(wk.objs.iter().any(|d| d == "m.ts:Plain"), "whole {wk:?}");
    let u = run(m, true);
    let uk = &field(tmpl(&u, "Box"), "f").kind;
    assert!(
        uk.top || (uk.objs.iter().any(|d| d == "m.ts:Plain") && uk.prims & wk.prims == wk.prims),
        "per unit Box.f {uk:?} covers whole {wk:?}\n{}",
        u.dump_text()
    );
}
