//! TOPC (2026-11-17): the for-of index walk over a typed Set or Map
//! (`SetValueAt`, `MapEntryKeyAt` / `MapEntryValueAt`, HIR
//! `lower_decl/body_stmt.rs`) reads the collection's own entry list
//! (`js_set_value_raw_at`, `js_map_entry_*_at`): its contents or
//! `undefined`, never a member of the collection's prototype chain.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// The verdict of the first site `.key` after `marker`, and the dump.
fn verdict(src: &str, key: &str, marker: &str) -> (Verdict, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()));
    (s.verdict.clone(), pf.dump_text())
}

// RED on base: the loop value of a for-of over a `Set<Obs>` is an Obs (or
// `undefined`), not Set.prototype's methods.
#[test]
fn topc_r1_set_for_of_value_is_an_element() {
    let src = "class Obs { n = 0; offer(k: number): void { this.n += k; } }\n\
               const all = new Set<Obs>();\n\
               all.add(new Obs());\n\
               for (const o of all) o.offer(2);\n\
               console.log(all.size);\n";
    let (v, dump) = verdict(src, "offer", "o.offer");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}\n{dump}");
}

// KEEP: a for-of over a `Map<string, Obs>` with a `[k, v]` head binds the
// entry's value (the same walk, `MapEntryValueAt`).
#[test]
fn topc_k1_map_for_of_entry_value() {
    let src = "class Obs { n = 0; offer(k: number): void { this.n += k; } }\n\
               const all = new Map<string, Obs>();\n\
               all.set(\"a\", new Obs());\n\
               for (const [k, o] of all) o.offer(k.length);\n\
               console.log(all.size);\n";
    let (v, dump) = verdict(src, "offer", "o.offer");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}\n{dump}");
}
