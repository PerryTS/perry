//! `class X extends <member read>`: the trailing property name of the
//! heritage (`O.Base`, `O["Base"]`) is a property key, not a binding, so it
//! names no class. The parent is the value the heritage evaluated to
//! (§17.2): `super(...)`, the implicit constructor's forwarded `super`, the
//! field initializers of the parent, `super.m()` and the inherited statics
//! all follow that value. A class elsewhere in the module that happens to
//! carry the property's name is unrelated (SEEDN1 m10: the implicit
//! constructor of a class expression stored in an object-literal property
//! was "known inactive" while Node ran it).

use crate::FactsConfig;

/// The active bodies of `src`: class bodies by name, functions by their own
/// name.
fn active_bodies(src: &str) -> Vec<String> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_hir::monomorphize_module(&mut hir);
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    pf.body_reach
        .bodies
        .iter()
        .map(|(_, i, n)| match hir.functions.iter().find(|f| f.id == *i) {
            Some(f) if n.is_empty() => f.name.clone(),
            _ => n.clone(),
        })
        .collect()
}

/// Body `name` is active: a function by its name, a class body by class and
/// body (`Inner::<fields>` matches the class expression `Inner__class_expr_3`).
fn runs(bodies: &[String], name: &str) -> bool {
    let (class, rest) = name.split_once("::").unwrap_or((name, ""));
    bodies.iter().any(|b| {
        let b = b.rsplit_once(".ts:").map_or(b.as_str(), |(_, x)| x);
        if rest.is_empty() {
            return b == name;
        }
        let Some((bc, br)) = b.split_once("::") else {
            return false;
        };
        br == rest && (bc == class || bc.starts_with(&format!("{class}__class_expr_")))
    })
}

/// The SEEDN1 reproducer: `Base` declared in `a` and the anonymous class
/// stored under the property `Base` in `b`.
#[test]
fn the_object_literal_class_a_member_heritage_reads_runs_its_fields() {
    let src = r#"
function a(tag: string) {
  class Base extends Error {}
  Base.prototype.name = tag;
  return class Tagged extends Base {};
}
const FA = a("A"); console.log(new FA("m").name);
function b(tag: string) {
  const O = { Base: class extends Error {} };
  O.Base.prototype.name = tag;
  return class Tagged extends O.Base {};
}
const FB = b("B"); console.log(new FB("m").name);
"#;
    let on = active_bodies(src);
    assert!(
        on.iter()
            .any(|b| b.contains("Base__anon") && b.ends_with("::<fields>")),
        "the stored class's fields inactive: {on:?}"
    );
}

/// Each heritage form constructs `Inner`, never the decoy class named like
/// the property (or the parameter): `Inner`'s fields, method and static
/// run; the decoy's do not.
const FORMS: &[(&str, &str)] = &[
    ("member", "const P = O.Base;\nclass X extends O.Base {"),
    ("computed-member", "const P = O.Base;\nclass X extends O[\"Base\"] {"),
    (
        "computed-member-key",
        "const k = [\"Base\"][0];\nclass X extends O[k] {",
    ),
    (
        "call-result",
        "function pick(): any { return O.Base; }\nclass X extends pick() {",
    ),
];

fn form_src(head: &str) -> String {
    format!(
        r#"
function decoyM() {{ return "decoy"; }}
function decoyS() {{ return "decoy"; }}
function innerM() {{ return "inner"; }}
function innerS() {{ return "inner"; }}
class Base {{ decoy = 1; m() {{ return decoyM(); }} static s() {{ return decoyS(); }} }}
const O: any = {{ Base: class Inner {{ inner = 1; m() {{ return innerM(); }} static s() {{ return innerS(); }} }} }};
{head}
  own = 2;
  m() {{ return super.m(); }}
}}
const x = new X();
console.log(x.inner, x.own, x.m(), (X as any).s(), new Base(), O);
"#
    )
}

#[test]
fn a_member_heritage_constructs_the_value_it_read() {
    for (name, head) in FORMS {
        let on = active_bodies(&form_src(head));
        for body in ["Inner::<fields>", "innerM", "innerS", "X::<fields>"] {
            assert!(runs(&on, body), "{name}: {body} inactive: {on:?}");
        }
        for body in ["decoyM", "decoyS"] {
            assert!(!runs(&on, body), "{name}: decoy {body} active: {on:?}");
        }
    }
}

/// A parameter heritage, with a module class named like the parameter.
#[test]
fn a_parameter_heritage_constructs_the_argument() {
    let src = r#"
class Base { decoy = 1; }
class Inner { inner = 1; }
function mk(Base: any) {
  return class extends Base {
    constructor(p: any) { super(p); }
  };
}
const M = mk(Inner);
console.log(new M(1).inner);
"#;
    let on = active_bodies(src);
    assert!(runs(&on, "Inner::<fields>"), "Inner fields inactive: {on:?}");
    assert!(!runs(&on, "Base::<fields>"), "decoy active: {on:?}");
}

/// Control: an identifier heritage names its binding; the parent is linked
/// statically as before, and an unrelated class stays inactive.
#[test]
fn an_identifier_heritage_keeps_its_static_parent() {
    let src = r#"
function baseM() { return "base"; }
function baseS() { return "base"; }
class Base { base = 1; m() { return baseM(); } static s() { return baseS(); } }
const O: any = { Other: class Other { other = 1; } };
class X extends Base { own = 2; m() { return super.m(); } }
const x = new X();
console.log(x.base, x.own, x.m(), X.s(), O);
"#;
    let on = active_bodies(src);
    for body in ["Base::<fields>", "baseM", "baseS", "X::<fields>"] {
        assert!(runs(&on, body), "{body} inactive: {on:?}");
    }
    assert!(!runs(&on, "Other::<fields>"), "Other active: {on:?}");
}
