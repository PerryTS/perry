// ONCHANGE1 (2026-11-15) rule U: a method key that a post-birth store adds on
// SOME birth sites of a class (`on(f) { this.cb = f }`) is read on the chain
// at a site whose receivers are only birth sites no store of the key reaches
// (ECMA-262 10.1.9: only [[Set]] / [[DefineOwnProperty]] add an own property;
// 10.1.8.1: OrdinaryGet then climbs the chain). Small JS, no three.js.
use super::*;
use perry_hir::Module;

fn facts(src: &str) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h = [(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// The verdicts of the `.cb` call sites of a.ts, in source order.
fn cb_calls(pf: &crate::ProgramFacts) -> Vec<crate::Verdict> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let mut v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == "cb" && s.is_call && !s.access.is_store())
        .map(|s| (s.byte_offset, s.verdict.clone()))
        .collect();
    v.sort_by_key(|x| x.0);
    v.into_iter().map(|x| x.1).collect()
}

fn proven(v: &crate::Verdict) -> bool {
    !matches!(v, crate::Verdict::No(_) | crate::Verdict::Throws)
}

const CLASS: &str = "let n = 0;
class E { cb() { n += 100; } on(f: () => void) { this.cb = f; return this; }
  fire() { this.cb(); } ping() { this.cb(); } mix() { this.cb(); } }
class H { e: E; constructor() { const e = new E(); e.on(() => { n++; }); this.e = e; } }
";

/// U1: `ping` sees only `p` (no `on` reaches it): proven (the prototype
/// method). `fire` (born-with only) and `mix` (both kinds) stay generic.
#[test]
fn onchange1_u1_unwritten_birth_site_reads_the_chain() {
    let src = format!(
        "{CLASS}const h = new H(); const p = new E();
const k = (process.argv[2] ?? \"a\").length;
for (let i = 0; i < k; i++) {{ h.e.fire(); p.ping(); h.e.mix(); p.mix(); }}
console.log(n);"
    );
    let v = cb_calls(&facts(&src));
    eprintln!("OC1 u1 {v:?}");
    assert_eq!(v.len(), 3, "{v:?}");
    assert!(!proven(&v[0]), "fire: born-with receivers, not rule U: {v:?}");
    assert!(proven(&v[1]), "ping: no store reaches p: {v:?}");
    assert!(!proven(&v[2]), "mix: both kinds: {v:?}");
}

/// U2 KEEP: `on` reaches `p` too (a second store site): `ping` stays generic.
#[test]
fn onchange1_u2_store_reaching_the_site_keeps() {
    let src = format!(
        "{CLASS}const h = new H(); const p = new E();
if (String(Math.random()) === \"x\") p.on(() => {{ n += 2; }});
const k = (process.argv[2] ?? \"a\").length;
for (let i = 0; i < k; i++) {{ h.e.fire(); p.ping(); }}
console.log(n);"
    );
    let v = cb_calls(&facts(&src));
    eprintln!("OC1 u2 {v:?}");
    assert!(v.iter().all(|x| !proven(x)), "{v:?}");
}

/// U3 KEEP: a computed-key store that may name `cb` reaches `p`.
#[test]
fn onchange1_u3_computed_store_keeps() {
    let src = format!(
        "{CLASS}const h = new H(); const p = new E();
const key = String(Math.random()) === \"x\" ? \"cb\" : \"zz\";
(p as any)[key] = () => {{ n += 3; }};
const k = (process.argv[2] ?? \"a\").length;
for (let i = 0; i < k; i++) {{ h.e.fire(); p.ping(); }}
console.log(n);"
    );
    let v = cb_calls(&facts(&src));
    eprintln!("OC1 u3 {v:?}");
    assert!(v.iter().all(|x| !proven(x)), "{v:?}");
}
