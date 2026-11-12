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
    assert_eq!(callee(&pf, "this.k.fill(1)", "fill"), Some(SITE_TYPED_ARRAY));
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
        ("d.fill(3)", "receiver:not-typed-array"),
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
