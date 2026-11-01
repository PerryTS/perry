//! A class accessor's receiver reaches its summarized body: what a getter
//! returns of `this` (a field's Map) is the getter's result, and what a
//! setter is given lands on `this`. Another module that writes into the
//! returned Map, or hands the setter a Map, changes what the class's own
//! reads see (`Model::group_summarized`).

use super::*;

/// `e.ts`: a search table in a public field. Its entries' reads are the
/// witness (`depth`, `score`).
fn engine(extra: &str) -> String {
    format!(
        r#"interface Entry {{ depth: number; score: number; }}
export class Eng {{
    tab = new Map<string, Entry>();
    search(key: string): number {{
        const k = "k" + key.length;
        const c = this.tab.get(k);
        if (c !== undefined) return c.depth + c.score;
        this.tab.set(k, {{ depth: 1, score: 2 }});
        return this.tab.size;
    }}
{extra}
}}"#
    )
}

const GETTER: &str = "    get table(): Map<string, Entry> { return this.tab; }";
const SETTER: &str = "    set table(m: Map<string, Entry>) { this.tab = m; }";

/// What another module puts into the Map: no layout of `e.ts`.
const FOREIGN_ENTRY: &str = r#"JSON.parse("{\"depth\":\"x\",\"score\":\"y\"}")"#;

fn entry_reads_proven(pf: &ProgramFacts) -> bool {
    ["depth", "score"].iter().all(|k| {
        let v: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.key == *k && pf.module_paths[s.module as usize] == "e.ts")
            .collect();
        assert!(!v.is_empty(), "no e.ts site {k}\n{}", pf.dump_text());
        v.iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
    })
}

fn modules(extra: &str, main: &str) -> (ProgramFacts, ProgramFacts) {
    let e = engine(extra);
    let m = format!("import {{ Eng }} from \"./e\";\nconst e = new Eng();\n{main}\nconsole.log(e.search(\"a\"));");
    let mods = [("e", e.as_str()), ("m", m.as_str())];
    let (u, w) = (units_of(&mods), whole_of(&mods));
    let stronger: Vec<_> = proven_set(&u)
        .difference(&proven_set(&w))
        .cloned()
        .collect();
    assert!(
        stronger.is_empty(),
        "per unit stronger than whole: {stronger:?}"
    );
    (u, w)
}

/// Before: the getter's summarized instance had no receiver, so its result
/// (`this.tab`) was empty, the `.set` had no receiver, and the whole
/// program proved `c.depth` on `e.ts`'s literal only.
#[test]
fn a_map_a_getter_returns_takes_another_modules_set() {
    for main in [
        format!("(e.table as any).set(\"k1\", {FOREIGN_ENTRY});"),
        format!("const t: any = e.table; t.set(\"k1\", {FOREIGN_ENTRY});"),
    ] {
        let (u, w) = modules(GETTER, &main);
        assert!(!entry_reads_proven(&w), "whole: {main}\n{}", w.dump_text());
        assert!(
            !entry_reads_proven(&u),
            "per unit: {main}\n{}",
            u.dump_text()
        );
    }
}

/// Before: the setter's summarized instance had no receiver, so `this.tab
/// = m` stored nowhere and the whole program never saw the new Map.
#[test]
fn a_map_a_setter_takes_becomes_the_field() {
    let main = format!("const m: any = new Map(); m.set(\"k1\", {FOREIGN_ENTRY}); e.table = m;");
    let (u, w) = modules(SETTER, &main);
    assert!(!entry_reads_proven(&w), "whole\n{}", w.dump_text());
    assert!(!entry_reads_proven(&u), "per unit\n{}", u.dump_text());
}

/// Negative controls: the getter's result reaches the other module, which
/// reads it, or writes an entry of the same layout: the entry reads stay
/// proven whole.
#[test]
fn a_map_a_getter_returns_stays_proven_without_a_foreign_entry() {
    for main in [
        "console.log(e.table.size);",
        "e.table.set(\"k1\", { depth: 3, score: 4 });",
    ] {
        let (_, w) = modules(GETTER, main);
        assert!(entry_reads_proven(&w), "whole: {main}\n{}", w.dump_text());
    }
}
