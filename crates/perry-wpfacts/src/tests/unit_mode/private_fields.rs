//! A class private field (`#x`) has no name outside its class body: no
//! other unit's code names it and no reflection lists it, so a published
//! layout's private field does not make what it holds leave
//! (`Model::private_atom`, `Model::private_field`). What it holds leaves
//! only as the class's own code lets it.

use super::*;

/// `e.ts`: a search table in a private field, read only inside the class.
/// Its entries' reads are the witness (`depth`, `score`).
fn engine(extra: &str) -> String {
    format!(
        r#"interface Entry {{ depth: number; score: number; }}
export class Eng {{
    #table = new Map<string, Entry>();
    search(key: string): number {{
        const k = "k" + key.length;
        const c = this.#table.get(k);
        if (c !== undefined) return c.depth + c.score;
        this.#table.set(k, {{ depth: 1, score: 2 }});
        return this.#table.size;
    }}
    reset(): void {{ this.#table.clear(); }}
{extra}
}}"#
    )
}

const ENTRY_KEYS: [&str; 2] = ["depth", "score"];

/// The modules lowered as the pipeline lowers them: class ids run on from
/// one module to the next, so no two classes of the program share one, nor
/// the storage key of a private field (`private_names::private_value_key`).
fn program_hirs(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
    let mut next_class_id = 1;
    mods.iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let (mut hir, next) =
                perry_hir::lower_module_with_class_id(&module, name, &path, next_class_id)
                    .expect("lowers");
            next_class_id = next;
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect()
}

/// `program_hirs` per unit and as one whole program.
fn units_and_whole(mods: &[(&str, &str)]) -> (ProgramFacts, ProgramFacts) {
    let hirs = program_hirs(mods);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    (
        crate::compute_units(&list, &FactsConfig::default()),
        crate::compute(&list, &FactsConfig::default()),
    )
}

fn e_sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && pf.module_paths[s.module as usize] == "e.ts")
        .collect();
    assert!(!v.is_empty(), "no e.ts site {key}\n{}", pf.dump_text());
    v
}

fn e_proven(pf: &ProgramFacts, key: &str) -> bool {
    e_sites(pf, key)
        .iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn entries_proven(pf: &ProgramFacts) -> bool {
    ENTRY_KEYS.iter().all(|k| e_proven(pf, k))
}

fn entries_refused(pf: &ProgramFacts) -> bool {
    ENTRY_KEYS.iter().all(|k| !e_proven(pf, k))
}

const MAIN: &str = r#"import { Eng } from "./e";
const e = new Eng();
console.log(e.search("a"), e.search("a"), Object.keys(e).length, JSON.stringify(e));
const s: any = { ...e };
for (const k in e) console.log(k, s[k]);
e.reset();"#;

/// The positive witness: per unit proves the entries' reads as the whole
/// program does, though another unit holds the instance and reflects on it.
/// Before: the initializer's Map never reached the field (no reader proved
/// even whole), and as a field of the published layout it would leave.
#[test]
fn a_private_fields_map_is_read_through_its_class_only() {
    let e = engine("");
    let mods = [("e", e.as_str()), ("m", MAIN)];
    let (u, w) = units_and_whole(&mods);
    assert!(entries_proven(&w), "whole\n{}", w.dump_text());
    assert!(entries_proven(&u), "per unit\n{}", u.dump_text());
    let stronger: Vec<_> = proven_set(&u)
        .difference(&proven_set(&w))
        .cloned()
        .collect();
    assert!(
        stronger.is_empty(),
        "per unit stronger than whole: {stronger:?}"
    );
}

/// What another unit puts into the Map once the class lets the Map out.
const FOREIGN_ENTRY: &str = r#"JSON.parse("{\"depth\":\"x\",\"score\":\"y\"}")"#;

/// The class's code returns the field's value: it leaves with the result.
/// Per unit refuses. The whole program wrongly proves the entries here, as
/// it does on main with a public field returned by a getter (another
/// unit's `.set` on the getter's Map is lost): lane SRM fixes that.
#[test]
#[ignore = "SRM"]
fn a_private_field_returned_by_its_class_leaves() {
    let e = engine("    get table(): Map<string, Entry> { return this.#table; }");
    let m = format!(
        r#"import {{ Eng }} from "./e";
const e = new Eng();
(e.table as any).set("k1", {FOREIGN_ENTRY});
console.log(e.search("a"));"#
    );
    let mods = [("e", e.as_str()), ("m", m.as_str())];
    let (u, w) = units_and_whole(&mods);
    for pf in [u, w] {
        assert!(entries_refused(&pf), "{}", pf.dump_text());
    }
}

/// The class's code stores the field's value under a public key.
#[test]
fn a_private_field_stored_under_a_public_key_leaves() {
    let e = engine("    pub: any = null;\n    expose(): void { this.pub = this.#table; }");
    let m = format!(
        r#"import {{ Eng }} from "./e";
const e = new Eng();
e.expose();
e.pub.set("k1", {FOREIGN_ENTRY});
console.log(e.search("a"));"#
    );
    let mods = [("e", e.as_str()), ("m", m.as_str())];
    let (u, w) = units_and_whole(&mods);
    for pf in [u, w] {
        assert!(entries_refused(&pf), "{}", pf.dump_text());
    }
}

/// The class's code hands the field's value to a function another unit
/// passes.
#[test]
fn a_private_field_passed_to_a_callers_function_leaves() {
    let e = engine("    share(hook: (m: Map<string, Entry>) => void): void { hook(this.#table); }");
    let m = format!(
        r#"import {{ Eng }} from "./e";
const e = new Eng();
e.share((t: any) => t.set("k1", {FOREIGN_ENTRY}));
console.log(e.search("a"));"#
    );
    let mods = [("e", e.as_str()), ("m", m.as_str())];
    let (u, w) = units_and_whole(&mods);
    for pf in [u, w] {
        assert!(entries_refused(&pf), "{}", pf.dump_text());
    }
}

/// A public key spelled as the private field's storage key is another
/// entry (the runtime keeps them apart): what the class's own code stores
/// under it never reaches the private field's readers.
#[test]
fn a_public_key_of_the_private_spelling_is_another_key() {
    let poke = |spelled: &str| {
        engine(&format!(
            r##"    poke(bad: any): void {{ (this as any)["{spelled}"] = bad; (this as any)["#table"] = bad; }}
    peek(): any {{ return (this as any)["{spelled}"]; }}"##
        ))
    };
    let id_of = |src: &str| {
        let hirs = program_hirs(&[("e", src)]);
        hirs[0]
            .1
            .classes
            .iter()
            .find(|c| c.name == "Eng")
            .expect("Eng")
            .id
    };
    let probe = poke("x");
    let spelled = perry_hir::private_names::private_value_key(id_of(&probe), "#table");
    let e = poke(&spelled);
    assert_eq!(id_of(&e), id_of(&probe));
    let m = format!(
        r#"import {{ Eng }} from "./e";
const e = new Eng();
const bad = new Map<string, any>();
bad.set("k1", {FOREIGN_ENTRY});
e.poke(bad);
console.log(e.search("a"));"#
    );
    let mods = [("e", e.as_str()), ("m", m.as_str())];
    let (u, w) = units_and_whole(&mods);
    for pf in [u, w] {
        assert!(entries_proven(&pf), "{}", pf.dump_text());
        // One row per key: the private field's has its claimed slot, the
        // public key of its spelling none.
        let eng = pf
            .templates
            .iter()
            .find(|t| t.display == "e.ts:Eng")
            .expect("Eng");
        let rows: Vec<_> = eng.fields.iter().filter(|f| f.key == spelled).collect();
        assert!(
            rows.iter().any(|f| f.private && f.slot == Some(0))
                && rows.iter().all(|f| f.private || f.slot.is_none()),
            "{}",
            pf.dump_text()
        );
    }
}

/// The completed layout codegen gives the instance (#11791,
/// `perry_hir::private_names::claimed_private_chain`): the birth keys, then
/// each private field at the next inline slot in declaration order. A read
/// of a private field proves as a read of a definite birth field does.
/// Before: no private field had a slot, its reads stayed unproven.
#[test]
fn a_private_field_is_read_at_its_claimed_slot() {
    let e = r#"export class Eng {
    a = 1;
    #t = new Map<string, number>();
    b = "x";
    #n = 3;
    run(k: string): number {
        this.#t.set(k, this.#n);
        return this.#t.size + this.#n + this.a;
    }
}"#;
    let m = r#"import { Eng } from "./e";
const e = new Eng();
console.log(e.run("q"), e.run("r"), Object.keys(e).length);"#;
    let mods = [("e", e), ("m", m)];
    let (u, w) = units_and_whole(&mods);
    for pf in [u, w] {
        let eng = pf
            .templates
            .iter()
            .find(|t| t.display == "e.ts:Eng")
            .expect("Eng");
        let slot = |name: &str| {
            eng.fields
                .iter()
                .find(|f| f.private && f.key.ends_with(&format!(":{name}>")))
                .and_then(|f| f.slot)
        };
        assert_eq!(
            (slot("#t"), slot("#n")),
            (Some(2), Some(3)),
            "{}",
            pf.dump_text()
        );
        let private_reads: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.key.starts_with("#<perry:private-value:"))
            .collect();
        assert_eq!(private_reads.len(), 4, "{}", pf.dump_text());
        assert!(
            private_reads
                .iter()
                .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set)),
            "{}",
            pf.dump_text()
        );
    }
}

/// A constructor that adds a key of its own: codegen has no completed
/// private layout (a claim's slot is not known), so neither has the facts.
#[test]
fn a_constructor_adding_keys_leaves_private_fields_without_a_slot() {
    let e = r#"export class Eng {
    #t = new Map<string, number>();
    constructor(on: boolean) { if (on) (this as any).extra = 1; }
    run(k: string): number { this.#t.set(k, 1); return this.#t.size; }
}"#;
    let m = r#"import { Eng } from "./e";
console.log(new Eng(true).run("q"), new Eng(false).run("r"));"#;
    let mods = [("e", e), ("m", m)];
    let (u, w) = units_and_whole(&mods);
    for pf in [u, w] {
        let eng = pf
            .templates
            .iter()
            .find(|t| t.display == "e.ts:Eng")
            .expect("Eng");
        assert!(
            eng.fields
                .iter()
                .filter(|f| f.private)
                .all(|f| f.slot.is_none()),
            "{}",
            pf.dump_text()
        );
    }
}

/// Another unit's subclass instance runs the class's initializer and
/// methods (its receiver is a marker there): the field it reads is what
/// the class's own code stored, never the subclass's same-named field.
#[test]
fn a_subclass_of_another_unit_finds_the_classs_own_private_field() {
    let e = engine("");
    let m = format!(
        r#"import {{ Eng }} from "./e";
class Sub extends Eng {{
    #table: any[] = [];
    poison(): void {{ this.#table.push({FOREIGN_ENTRY}); }}
}}
const s = new Sub();
s.poison();
console.log(s.search("a"), s.search("a"));"#
    );
    let mods = [("e", e.as_str()), ("m", m.as_str())];
    let (u, w) = units_and_whole(&mods);
    assert!(entries_proven(&w), "whole\n{}", w.dump_text());
    assert!(entries_proven(&u), "per unit\n{}", u.dump_text());
}

/// A static private field is the class object's, keyed apart from its
/// public statics; a brand check reads no field.
#[test]
fn a_static_private_field_holds_its_initializer() {
    let e = r#"interface Entry { depth: number; score: number; }
export class Eng {
    static #shared = new Map<string, Entry>();
    static depth = 0;
    static note(k: string): number {
        const key = "k" + k.length;
        const c = Eng.#shared.get(key);
        if (c !== undefined) return c.depth + c.score;
        Eng.#shared.set(key, { depth: 1, score: 2 });
        return 0;
    }
    static has(o: object): boolean { return #shared in o; }
}"#;
    let m = r#"import { Eng } from "./e";
console.log(Eng.note("a"), Eng.note("a"), Eng.has(Eng), Eng.has({}));"#;
    let mods = [("e", e), ("m", m)];
    let (u, w) = units_and_whole(&mods);
    for pf in [u, w] {
        assert!(entries_proven(&pf), "{}", pf.dump_text());
    }
}
