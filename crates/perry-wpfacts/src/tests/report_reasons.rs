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
export function make(kind: number): Node { return new Node(kind); }
export function kindOf(n: Node): number { return n.kind; }
";

#[test]
fn a_store_of_an_undeclared_key_reports_written_and_the_key() {
    // `b` stores `parent`, a key `Node` never declares, into the instances
    // it gets from `make`: no store effect covers it (decision 17), so `b`
    // reports `a.ts:Node`, and the record says why.
    let reader = "
import { make, kindOf } from \"./a\";
export function link(k: number): number {
    const n = make(k);
    const p = make(k + 1);
    (n as any).parent = p;
    return kindOf(n);
}
";
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
    let reader = "
import { make, kindOf } from \"./a\";
export function bump(k: number): number {
    const n = make(k);
    n.kind = k + 1;
    return kindOf(n);
}
";
    let on = units(&[("a", OWNER), ("b", reader)], true);
    assert!(!on.broken.iter().any(|d| d == "a.ts:Node"), "{:?}", on.broken);
    assert!(reports(&on).iter().all(|l| !l.contains("a.ts:Node")), "{:?}", reports(&on));
}
