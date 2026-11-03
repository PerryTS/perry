//! CHB11: a number key names the own fields of a program object a number
//! may name (DECISIONS 217, `Model::num_fields`), never a named one. A
//! marker has no own fields (`Model::field`: its fields are its reach
//! markers): what a number key reads from one is its `[]` path step, which
//! `IdxLoad` edges in under the path's loop collapse (DECISIONS 158). The
//! own-field edge on a marker stood outside that collapse and made a marker
//! per iteration of `x = x.next[0]` (tssrc per unit: 27k markers, 25 GB).
use super::*;

fn facts_of(src: &str, per_unit: bool, max_pts: Option<u64>) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    cfg.max_pts = max_pts;
    match per_unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

/// The verdict of the one site of `key` that follows `marker` in `src`.
fn verdict_at(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> Verdict {
    let at = src.find(marker).expect("marker") as u32;
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()))
        .verdict
        .clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// The unit summaries of a per-unit run: `(path, nodes, pts, aborted)`.
fn unit_rows(pf: &ProgramFacts) -> Vec<(String, usize, u64, bool)> {
    pf.units
        .iter()
        .map(|(p, s)| (p.clone(), s.nodes, s.pts_entries, s.aborted))
        .collect()
}

/// `concatenateDiagnosticMessageChains` (TypeScript compiler/utilities.ts):
/// a linked-list walk through an element of a field, `x = x.next[0]`, on a
/// parameter; whole has concrete, finite chain objects.
const CHAIN_WALK: &str = "interface Chain { text: string; next?: Chain[] }\n\
     function concat(head: Chain, tail: Chain): void {\n\
         let last = head;\n\
         while (last.next) { last = last.next[0]; }\n\
         last.next = [tail];\n\
     }\n\
     function make(text: string, next?: Chain): Chain {\n\
         return { text, next: next === undefined ? undefined : [next] };\n\
     }\n\
     const head = make(\"a\", make(\"b\"));\n\
     concat(head, make(\"c\"));\n\
     let n = 0;\n\
     for (let c: Chain | undefined = head; c !== undefined; c = c.next === undefined ? undefined : c.next[0]) n += c.text.length;\n\
     console.log(n);\n";

/// A points-to budget far above what the walks need with finite markers
/// (`max_pts / 8` nodes): a marker per iteration runs past it.
const WALK_BUDGET: u64 = 8 * 5_000;

/// Every unit of a per-unit run of `src` solved within `WALK_BUDGET`, and
/// whole did too.
fn finite(src: &str) {
    let pf = facts_of(src, true, Some(WALK_BUDGET));
    let rows = unit_rows(&pf);
    assert!(!rows.is_empty(), "{}", pf.dump_text());
    assert!(
        rows.iter().all(|r| !r.3) && !pf.stats.aborted,
        "a unit ran past the budget: {rows:?}"
    );
    let whole = facts_of(src, false, Some(WALK_BUDGET));
    assert!(!whole.stats.aborted, "whole: {:?}", whole.stats);
}

/// `x = x[0]` on a parameter: a number key read on the parameter marker
/// itself. Its any-key field holds its reach marker, and a marker arriving
/// at a field node that is not the marker's own (`Model::crosses`) becomes
/// a member of a fresh node marker, whose own number key read repeats the
/// step: with an own-field node per marker, a marker per iteration.
const SELF_WALK: &str = "function innermost(p: any, depth: number): any {\n\
         let x = p;\n\
         for (let i = 0; i < depth; i++) { x = x[0]; }\n\
         return x;\n\
     }\n\
     console.log(innermost([[[\"a\"]]], 3), innermost([\"b\"], 1));\n";

#[test]
fn number_key_on_a_parameter_marker_is_finite() {
    finite(SELF_WALK);
}

/// Per unit, `head` is a parameter marker: `last.next[0]` reads its `[]`
/// path step and the walk folds back onto the `next` step, so the unit's
/// solve stays within a small budget. Whole has the same bound.
#[test]
fn number_key_walk_on_a_marker_is_finite() {
    finite(CHAIN_WALK);
}

/// Per unit never claims a site whole does not: the marker's `[]` step is
/// unknown, whole's concrete chain is not.
#[test]
fn per_unit_walk_is_never_stronger_than_whole() {
    let whole = facts_of(CHAIN_WALK, false, None);
    let units = facts_of(CHAIN_WALK, true, None);
    assert!(!whole.stats.aborted && !units.stats.aborted);
    for s in units.sites.iter().filter(|s| proven(&s.verdict)) {
        let w = whole
            .sites
            .iter()
            .find(|t| t.byte_offset == s.byte_offset && t.key == s.key)
            .unwrap_or_else(|| panic!("no whole site for {}@{}", s.key, s.byte_offset));
        assert!(
            proven(&w.verdict),
            "per unit stronger at .{}@{}: {:?} vs whole {:?}",
            s.key,
            s.byte_offset,
            s.verdict,
            w.verdict
        );
    }
    // Whole proves the walk over its concrete chain objects.
    assert!(proven(&verdict_at(
        &whole,
        CHAIN_WALK,
        "n += c.text",
        "length"
    )));
}

/// A number key on a program object reads its numeric fields and nothing
/// under a named key: `o[i]` is a String although `o.name` is unknown.
const OWN_NUM: &str = "function f(): number {\n\
         const o: any = { 0: \"a\", 1: \"b\", name: JSON.parse(\"{}\") };\n\
         let n = 0;\n\
         for (let i = 0; i < 2; i++) n += (o[i] ?? \"\").length;\n\
         return n;\n\
     }\n\
     function g(k: string): number {\n\
         const o: any = { 0: \"a\", 1: \"b\", name: JSON.parse(\"{}\") };\n\
         return (o[k] ?? \"\").length;\n\
     }\n\
     console.log(f(), g(\"name\"));\n";

#[test]
fn number_key_on_a_program_object_reads_its_numeric_fields_only() {
    for per_unit in [false, true] {
        let pf = facts_of(OWN_NUM, per_unit, None);
        let num = verdict_at(&pf, OWN_NUM, "(o[i]", "length");
        assert!(proven(&num), "per_unit={per_unit} o[i].length: {num:?}");
        // The control: a string key reads every own field, `name` among them.
        let named = verdict_at(&pf, OWN_NUM, "(o[k]", "length");
        assert!(
            !proven(&named),
            "per_unit={per_unit} o[k].length: {named:?}"
        );
    }
}
