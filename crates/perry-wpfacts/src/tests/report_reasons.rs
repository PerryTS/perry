//! The census reports carry their reasons into the dump (design §11.3):
//! per broken display a `report` record names its owners, the round it
//! broke in and each reporting unit with why it reported (`written`, `adds
//! keys`, `escaped`, `hard`, `aborted`, …). Dump only: the facts do not
//! change with the dump off.

use super::*;

fn hirs_of(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
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

fn units(mods: &[(&str, &str)], diagnostics: bool) -> ProgramFacts {
    let h = hirs_of(mods);
    let list: Vec<(String, &perry_hir::Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        diagnostics,
        ..FactsConfig::default()
    };
    crate::compute_units(&list, &cfg)
}

/// The `report` records of the dump, by display.
fn reports(pf: &ProgramFacts) -> Vec<String> {
    pf.dump_text()
        .lines()
        .filter(|l| l.starts_with("D {") && l.contains("\"record\":\"report\""))
        .map(str::to_owned)
        .collect()
}

const OWNER: &str = "
export class Node {
    kind: number;
    constructor(kind: number) { this.kind = kind; }
}
export function kindOf(n: Node): number { return n.kind; }
";

#[test]
fn a_store_of_an_undeclared_key_reports_written_and_the_key() {
    // `b` stores `parent`, a key `Node` never declares, into a `Node`: no
    // store effect covers it (decision 17), so `b` reports `a.ts:Node`, and
    // the record says why.
    let reader = "import { Node, kindOf } from './a'; const n = new Node(1); (n as any).parent = new Node(2); console.log(kindOf(n));";
    let mods = [("a", OWNER), ("b", reader)];
    let on = units(&mods, true);
    assert!(on.broken.iter().any(|d| d == "a.ts:Node"), "{:?}", on.broken);
    let rs = reports(&on);
    let r = rs
        .iter()
        .find(|l| l.contains("\"display\":\"a.ts:Node\""))
        .unwrap_or_else(|| panic!("no report record for a.ts:Node\n{}", rs.join("\n")));
    assert!(r.contains("\"owners\":[\"a.ts"), "{r}");
    assert!(r.contains("\"unit\":\"b.ts"), "{r}");
    assert!(r.contains("written: parent (class undeclared key)"), "{r}");
    assert!(r.contains("adds keys: parent"), "{r}");
    // Dump only: the same facts with the dump off, and no records.
    let off = units(&mods, false);
    assert_eq!(off.broken, on.broken);
    assert!(reports(&off).is_empty());
    assert_eq!(
        off.sites.iter().map(|s| format!("{}:{:?}", s.byte_offset, s.verdict)).collect::<Vec<_>>(),
        on.sites.iter().map(|s| format!("{}:{:?}", s.byte_offset, s.verdict)).collect::<Vec<_>>()
    );
}

#[test]
fn a_store_of_a_definite_key_is_a_store_effect_and_no_report() {
    // The control: `kind` is definite in every `Node`; the store is the
    // store effect decision 17 publishes, nothing is broken or reported.
    let reader = "import { Node, kindOf } from './a'; const n = new Node(1); n.kind = 2; console.log(kindOf(n));";
    let on = units(&[("a", OWNER), ("b", reader)], true);
    assert!(!on.broken.iter().any(|d| d == "a.ts:Node"), "{:?}", on.broken);
    assert!(reports(&on).iter().all(|l| !l.contains("a.ts:Node")), "{:?}", reports(&on));
}


fn whole(mods: &[(&str, &str)]) -> ProgramFacts {
    let h = hirs_of(mods);
    let list: Vec<(String, &perry_hir::Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    crate::compute(&list, &FactsConfig::default())
}

fn verdicts_at(pf: &ProgramFacts, module: &str, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == module).expect("module");
    pf.sites
        .iter()
        .filter(|s| s.module as usize == mi && s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

/// The `report` record of `display`, panicking with all records when absent.
fn report_of(pf: &ProgramFacts, display: &str) -> String {
    let rs = reports(pf);
    rs.iter()
        .find(|l| l.contains(&format!("\"display\":\"{display}\"")))
        .cloned()
        .unwrap_or_else(|| panic!("no report record for {display}\n{}", rs.join("\n")))
}

fn literal_display(pf: &ProgramFacts, module: &str, key: &str) -> String {
    pf.broken
        .iter()
        .find(|d| d.starts_with(&format!("{module}:__AnonShape_")))
        .cloned()
        .unwrap_or_else(|| panic!("no broken literal of {module} (key {key}): {:?}", pf.broken))
}

// UED (2026-10-30, nightly/CW/UED_DESIGN_2026-10-30.md): the three reasons
// that hold most "broken layout of another unit" sites on tssrc, each in
// its smallest program. Each pins today's per-unit result beside the whole
// program's; the design's options change the per-unit side only.

#[test]
fn ued_r1_a_call_of_an_unknown_value_in_a_body_no_unit_runs_breaks_per_unit() {
    // tssrc `Project::enableProxy`: `pluginModuleFactory({ typescript: ts })`
    // in a method only tsserver's plugin loader runs. Here `q.load` passes
    // an unknown value to `P.enable`, and nothing in the program runs
    // `load`. Per unit `load` runs for any caller: `enable` calls ⊤, the
    // literal holding the namespace escapes (q reports it), `p` publishes
    // the namespace described by no contract (203), and `a`'s `D` breaks.
    // The whole program never walks `load` or `enable`: `D.code` is T0.
    let q = "'";
    let a = "export const D = { code: 1 }; export function first(): number { return D.code; }";
    let p = format!("import * as a from {q}./a{q}; export class P {{ enable(f: any): any {{ return f({{ typescript: a }}); }} }}");
    let qq = format!("import {{ P }} from {q}./p{q}; export function load(name: string): void {{ new P().enable((globalThis as any)[name]); }}");
    let main = format!("import {{ first }} from {q}./a{q}; import {{ P }} from {q}./p{q}; import {{ load }} from {q}./q{q}; console.log(first(), typeof P, typeof load);");
    let mods = [("a", a), ("p", p.as_str()), ("q", qq.as_str()), ("main", main.as_str())];
    let u = units(&mods, true);
    let d = literal_display(&u, "a.ts", "code");
    let r = report_of(&u, &d);
    assert!(r.contains("\"unit\":\"p.ts#"), "{r}");
    assert!(r.contains("escaped: published to another unit, described by no contract"), "{r}");
    let pd = literal_display(&u, "p.ts", "typescript");
    let r = report_of(&u, &pd);
    assert!(r.contains("\"unit\":\"q.ts#"), "{r}");
    assert!(r.contains("escaped: passed to a call of an unknown function value"), "{r}");
    assert!(verdicts_at(&u, "a.ts", "code").iter().all(|v| v.starts_with("No")));
    assert_eq!(verdicts_at(&whole(&mods), "a.ts", "code"), ["T0Mono"]);
}

#[test]
fn ued_r2_a_method_read_from_a_field_set_through_a_bare_parameter_escapes_its_receiver() {
    // tssrc `objectAllocator`: `setObjectAllocator(alloc)` writes the
    // literal's fields from a parameter of a plain published function (a
    // bare marker, UNIT_EDGE_AUDIT §3 A), so they are published unknown;
    // the checker's `objectAllocator.getSymbolConstructor()` calls ⊤ and its
    // receiver escapes: the reader's report breaks the literal. The whole
    // program knows `make` is the closure `c` passes.
    let q = "'";
    let a = "export const alloc = { make: (): number => 1 }; export function setMake(f: () => number): void { alloc.make = f; }";
    let c = format!("import {{ setMake }} from {q}./a{q}; setMake(() => 2);");
    let b = format!("import {{ alloc }} from {q}./a{q}; import {q}./c{q}; console.log(alloc.make());");
    let mods = [("a", a), ("c", c.as_str()), ("b", b.as_str())];
    let u = units(&mods, true);
    let d = literal_display(&u, "a.ts", "make");
    let r = report_of(&u, &d);
    assert!(r.contains("\"unit\":\"b.ts#"), "{r}");
    assert!(r.contains("escaped: receiver of a call of an unknown function value @b.ts|"), "{r}");
    assert!(verdicts_at(&u, "b.ts", "make").iter().all(|v| v.starts_with("No")));
    assert_eq!(verdicts_at(&whole(&mods), "b.ts", "make"), ["T0Set"]);
}

#[test]
fn ued_r3_an_argument_to_another_units_escaping_parameter_breaks_only_when_run() {
    // tssrc `loadNodeModuleFromDirectory`: an argument passed to another
    // unit's function whose contract lets the parameter escape. In a body
    // nothing runs (`later`), per unit still reports it; the whole program
    // proves `S.k`. Where the call runs (`send` at the top level), the
    // escape is real and both modes lose the site.
    let q = "'";
    let a = "export const S = { k: 1 }; export function readK(): number { return S.k; }";
    let c = "export function put(x: any): void { (globalThis as any).slot = x; }";
    let b = format!("import {{ S, readK }} from {q}./a{q}; import {{ put }} from {q}./c{q}; export function later(): void {{ put(S); }} console.log(readK());");
    let mods = [("a", a), ("c", c), ("b", b.as_str())];
    let u = units(&mods, true);
    let r = report_of(&u, &literal_display(&u, "a.ts", "k"));
    assert!(r.contains("escaped: passed to another unit's function @b.ts|"), "{r}");
    assert!(verdicts_at(&u, "a.ts", "k").iter().all(|v| v.starts_with("No")));
    assert_eq!(verdicts_at(&whole(&mods), "a.ts", "k"), ["T0Mono"]);
    // Run at the top level: the escape is the program's, whole agrees.
    let b = format!("import {{ S, readK }} from {q}./a{q}; import {{ put }} from {q}./c{q}; put(S); console.log(readK());");
    let ran = [("a", a), ("c", c), ("b", b.as_str())];
    assert!(verdicts_at(&whole(&ran), "a.ts", "k").iter().all(|v| v.starts_with("No")));
}
