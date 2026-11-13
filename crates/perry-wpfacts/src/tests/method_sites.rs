//! Method sites (`method_sites`): what the solve proves of the receiver of
//! a by-name method call.

use crate::FactsConfig;
use perry_hir::folded_call::{Site, SITE_TYPED_ARRAY};

const SRC: &str = r#"
class Sub extends Float64Array { tag(): string { return "s"; } }
class Holder { k: Float64Array = new Float64Array(2); run(): any { return this.k.fill(1); } }
function real(a: Float64Array): any { return a.fill(1); }
function folded(b: Float64Array): any { return b.at(0); }
function lied(c: Float64Array): any { return c.fill(2); }
function sub(d: Float64Array): any { return d.fill(3); }
function nullish(e: Float64Array): any { return e.fill(4); }
function mixed(f: Float64Array): any { return f.at(1); }
const t = new Float64Array(4);
real(t);
folded(t);
lied({ fill() { return "F"; } } as any);
sub(new Sub(2));
nullish(null as any);
mixed(t);
mixed([1, 2] as any);
new Holder().run();
"#;

fn solve(sabotage: Option<&str>) -> crate::ProgramFacts {
    let module = perry_parser::parse_typescript(SRC, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        diagnostics: true,
        max_work: Some(50_000_000),
        sabotage: sabotage.map(str::to_string),
        ..FactsConfig::default()
    };
    crate::compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// The dump lines of the sites of method `key` whose line contains `part`.
fn lines(pf: &crate::ProgramFacts, key: &str, part: &str) -> Vec<String> {
    let key = format!(" key={key} ");
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# method_site ") && l.contains(&key) && l.contains(part))
        .cloned()
        .collect()
}

/// The exported record of the callee site at `text` (SWC offsets are
/// 1-based).
fn callee(pf: &crate::ProgramFacts, text: &str, key: &str) -> Option<u8> {
    assert_eq!(SRC.matches(text).count(), 1, "`{text}` names one site");
    let site = Site::Callee(SRC.find(text).expect("site in source") as u32 + 1);
    pf.bindings_for("t.ts")
        .expect("module bindings")
        .method_sites
        .iter()
        .find(|(s, k, _)| *s == site && k == key)
        .map(|(_, _, flags)| *flags)
}

/// A receiver that holds only realm-born typed arrays has the fact: an
/// unfolded call by its callee offset, through a parameter and a field.
#[test]
fn a_typed_array_receiver_has_the_fact() {
    let pf = solve(None);
    assert_eq!(callee(&pf, "a.fill(1)", "fill"), Some(SITE_TYPED_ARRAY));
    assert_eq!(
        callee(&pf, "this.k.fill(1)", "fill"),
        Some(SITE_TYPED_ARRAY)
    );
}

/// A folded node (`b.at(0)` is `Expr::ArrayAt`) has a site of its own, named
/// by the local its receiver reads.
#[test]
fn a_folded_node_has_a_site() {
    let pf = solve(None);
    let at = lines(&pf, "at", "site=local:");
    assert!(
        at.iter().any(|l| l.contains(" typed_array=1 ")),
        "the folded `b.at(0)` is a proven site: {at:?}"
    );
    assert!(
        at.iter().any(|l| l.contains(" typed_array=0(")),
        "the folded `f.at(1)` also holds an Array: {at:?}"
    );
}

/// The annotation proves nothing: an ordinary object, a subclass instance,
/// `null` and a mixed receiver have no fact, each with its reason.
#[test]
fn a_lying_annotation_has_no_fact() {
    let pf = solve(None);
    for (text, why) in [
        ("c.fill(2)", "receiver:not-typed-array"),
        ("d.fill(3)", "receiver:dropped"),
        ("e.fill(4)", "receiver:primitive"),
    ] {
        assert_eq!(callee(&pf, text, "fill"), None, "{text}");
        let offset = SRC.find(text).unwrap() + 1;
        let l = lines(&pf, "fill", &format!("site=callee:{offset} "));
        assert!(
            l.len() == 1 && l[0].contains(&format!(" typed_array=0({why}) ")),
            "{text}: {l:?}"
        );
    }
}

/// The negative control claims every site.
#[test]
fn the_sabotage_claims_every_site() {
    let pf = solve(Some("tasite"));
    for text in ["c.fill(2)", "d.fill(3)", "e.fill(4)"] {
        assert_eq!(callee(&pf, text, "fill"), Some(SITE_TYPED_ARRAY), "{text}");
    }
}

/// The Array-member fact (`SITE_ARRAY_MEMBER`): one method name per shape.
mod array_member {
    use crate::FactsConfig;

    const SRC: &str = r#"
class H { k: number[] = [1, 2, 3]; run(): any { return this.k.slice(1); } }
function folded(a: number[]): number { return a.indexOf(1); }
function chain(b: number[]): any { return b.fill(0); }
function own(c: number[]): any { return c.lastIndexOf(2); }
function lied(d: number[]): any { return d.includes(1); }
function nullish(e: number[]): any { return e.join(","); }
function popped(f: number[]): any { return f.pop(); }
const xs = [1, 2, 3];
folded(xs);
chain(xs);
popped(xs);
const o: any = [4, 5];
o.lastIndexOf = () => "OWN";
own(o);
lied({ length: 0, includes() { return "FAKE"; } } as any);
nullish(null as any);
new H().run();
"#;

    fn solve(sabotage: Option<&str>) -> crate::ProgramFacts {
        let module = perry_parser::parse_typescript(SRC, "t.ts").expect("parses");
        let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
        let cfg = FactsConfig {
            diagnostics: true,
            max_work: Some(50_000_000),
            sabotage: sabotage.map(str::to_string),
            ..FactsConfig::default()
        };
        crate::compute(&[("t.ts".to_string(), &hir)], &cfg)
    }

    /// The one dump line of the site of method `key`.
    fn line(pf: &crate::ProgramFacts, key: &str) -> String {
        let key = format!(" key={key} ");
        let l: Vec<&String> = pf
            .rep_probe
            .iter()
            .filter(|l| l.starts_with("# method_site ") && l.contains(&key))
            .collect();
        assert_eq!(l.len(), 1, "one site of{key}: {l:?}");
        l[0].clone()
    }

    /// An Array the program bore, with no own key of the name and no
    /// prototype write: the folded node by its local, the folded node by its
    /// receiver read, the unfolded call by its callee, `Expr::ArrayPop`.
    #[test]
    fn a_plain_array_site_is_a_member_site() {
        let pf = solve(None);
        for (key, site) in [
            ("indexOf", "site=local:"),
            ("slice", "site=callee:"),
            ("fill", "site=callee:"),
            ("pop", "site=local:"),
        ] {
            let l = line(&pf, key);
            assert!(l.contains(site) && l.ends_with(" array_member=1"), "{l}");
        }
    }

    /// An own property of the name, an object behind the annotation and
    /// `null` are no member sites, each with its reason.
    #[test]
    fn an_unproven_receiver_is_no_member_site() {
        let pf = solve(None);
        for (key, why) in [
            ("lastIndexOf", "array_member=0(member:own-or-undescribed)"),
            ("includes", "array_member=0(receiver:not-array)"),
            ("join", "array_member=0(receiver:primitive)"),
        ] {
            let l = line(&pf, key);
            assert!(l.ends_with(why), "{l}");
        }
        let exported = &pf.bindings_for("t.ts").expect("bindings").method_sites;
        for key in ["lastIndexOf", "includes", "join"] {
            assert!(
                !exported.iter().any(|(_, k, f)| k == key
                    && f & perry_hir::folded_call::SITE_ARRAY_MEMBER != 0),
                "{key} is exported as a member site"
            );
        }
    }

    /// The negative control claims every site.
    #[test]
    fn the_sabotage_claims_every_site() {
        let pf = solve(Some("arraysite"));
        for key in ["lastIndexOf", "includes", "join"] {
            let l = line(&pf, key);
            assert!(l.ends_with(" array_member=1"), "{l}");
        }
    }
}

/// Map / Set receivers (`SITE_MAP`, `SITE_SET`): a field that only ever holds
/// a realm-born Map has the Map fact at its folded `get` / `set` sites; a
/// parameter that is also handed an Array, an object or a Map subclass
/// instance has none; the sabotage claims every site.
mod collections {
    use crate::FactsConfig;
    use perry_hir::folded_call::{SITE_MAP, SITE_SET};

    const SRC: &str = r#"
class Sub extends Map<number, number> {}
class Holder {
  table: Map<string, number> = new Map<string, number>();
  seen: Set<number> = new Set<number>();
  run(k: string): any { this.table.set(k, 1); this.seen.add(2); return this.table.get(k); }
}
function onlyMaps(m: Map<number, number>): any { return m.get(1); }
function lied(m: Map<number, number>): any { return m.get(2); }
function subbed(m: Map<number, number>): any { return m.get(3); }
function sets(s: Set<number>): any { return s.has(4); }
new Holder().run("a");
onlyMaps(new Map<number, number>());
lied(new Map<number, number>());
lied([1, 2] as any);
lied({ get() { return 0; } } as any);
subbed(new Sub());
sets(new Set<number>());
sets(new Map<number, number>() as any);
"#;

    fn solve(sabotage: Option<&str>) -> crate::ProgramFacts {
        let module = perry_parser::parse_typescript(SRC, "t.ts").expect("parses");
        let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
        let cfg = FactsConfig {
            diagnostics: true,
            max_work: Some(50_000_000),
            sabotage: sabotage.map(str::to_string),
            ..FactsConfig::default()
        };
        crate::compute(&[("t.ts".to_string(), &hir)], &cfg)
    }

    /// The flags of every exported site of method `key`, sorted.
    fn flags(pf: &crate::ProgramFacts, key: &str) -> Vec<u8> {
        let mut out: Vec<u8> = pf
            .bindings_for("t.ts")
            .expect("module bindings")
            .method_sites
            .iter()
            .filter(|(_, k, _)| k == key)
            .map(|(_, _, f)| *f & (SITE_MAP | SITE_SET))
            .collect();
        out.sort_unstable();
        out
    }

    fn dump(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
        let key = format!(" key={key} ");
        pf.rep_probe
            .iter()
            .filter(|l| l.starts_with("# method_site ") && l.contains(&key))
            .cloned()
            .collect()
    }

    #[test]
    fn a_receiver_of_realm_born_collections_has_the_fact() {
        let pf = solve(None);
        // `this.table.set` and `this.seen.add`: one site each, proven.
        assert!(
            flags(&pf, "set").contains(&SITE_MAP),
            "{:?}",
            dump(&pf, "set")
        );
        assert!(
            flags(&pf, "add").contains(&SITE_SET),
            "{:?}",
            dump(&pf, "add")
        );
        // `get`: `this.table.get` and `onlyMaps` proven; `lied` (an Array, an
        // object) and `subbed` (a subclass instance) are not.
        let gets = dump(&pf, "get");
        assert_eq!(
            gets.iter().filter(|l| l.contains(" map=1 ")).count(),
            2,
            "{gets:?}"
        );
        assert_eq!(
            gets.iter().filter(|l| l.contains(" map=0(")).count(),
            2,
            "{gets:?}"
        );
        // `sets` also receives a Map: no Set fact.
        // A site without any fact is not exported.
        assert!(flags(&pf, "has").is_empty(), "{:?}", dump(&pf, "has"));
        assert!(
            dump(&pf, "has").iter().all(|l| l.contains(" set=0(")),
            "{:?}",
            dump(&pf, "has")
        );
    }

    #[test]
    fn the_sabotage_claims_every_site() {
        let pf = solve(Some("collsite"));
        for key in ["get", "has"] {
            for l in dump(&pf, key) {
                assert!(l.contains(" map=1 ") && l.contains(" set=1 "), "{l}");
            }
        }
    }
}
