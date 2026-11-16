// ONCHG2 (2026-11-16): a proven named method call whose callee set is only
// compiled closures records them as bodies (`SiteFacts::target_bodies`), the
// set codegen calls through the loaded function object (ECMA-262 13.3.6.2
// EvaluateCall, 10.2.1 [[Call]]). A set holding a method, a builtin or an
// unknown value records none. Small JS, unknowns from `process.argv[2]`.
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

/// (verdict, number of target bodies) of the `.cb` call sites in `fire`.
fn cb_calls(pf: &crate::ProgramFacts) -> Vec<(crate::Verdict, usize)> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let mut v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == "cb" && s.is_call && !s.access.is_store())
        .map(|s| (s.byte_offset, s.verdict.clone(), s.target_bodies.len()))
        .collect();
    v.sort_by_key(|x| x.0);
    v.into_iter().map(|x| (x.1, x.2)).collect()
}

const CLASS: &str = "let n = 0; const K = Number(process.argv[2]);
class E { cb() { n += 100; } on(f: () => void) { this.cb = f; return this; } fire() { this.cb(); } }
";

/// RED: two born-with arrows (two bodies, two environments each): T0Set, two bodies.
#[test]
fn onchg2_red_two_arrows_are_bodies() {
    let src = format!(
        "{CLASS}class H {{ e: E; constructor(k: number) {{ const e = new E(); e.on(() => {{ n += k; }}); this.e = e; }} }}
class G {{ e: E; constructor() {{ const e = new E(); e.on(() => {{ n *= 2; }}); this.e = e; }} }}
const hs = [new H(K), new H(K + 1)]; const g = new G();
for (let i = 0; i < 3; i++) {{ hs[i % 2].e.fire(); g.e.fire(); }}
console.log(n);"
    );
    let pf = facts(&src);
    let v = cb_calls(&pf);
    assert!(
        v.iter().any(|(vd, b)| matches!(vd, crate::Verdict::T0Set) && *b == 2),
        "{v:?}\n{}",
        pf.dump_text()
    );
}

/// RED: one arrow: T0Mono, one body.
#[test]
fn onchg2_red_one_arrow_is_one_body() {
    let src = format!(
        "{CLASS}class H {{ e: E; constructor(k: number) {{ const e = new E(); e.on(() => {{ n += k; }}); this.e = e; }} }}
const hs = [new H(K), new H(K + 1)];
for (let i = 0; i < 3; i++) hs[i % 2].e.fire();
console.log(n);"
    );
    let pf = facts(&src);
    let v = cb_calls(&pf);
    assert!(
        v.iter().any(|(vd, b)| matches!(vd, crate::Verdict::T0Mono) && *b == 1),
        "{v:?}\n{}",
        pf.dump_text()
    );
}

/// KEEP: an instance that never gets `on`: the prototype method is in the
/// set, which is then no set of plain bodies: no bodies recorded.
#[test]
fn onchg2_keep_method_member_records_none() {
    let src = format!(
        "{CLASS}class H {{ e: E; constructor(k: number) {{ const e = new E(); e.on(() => {{ n += k; }}); this.e = e; }} }}
const es = [new H(K).e, new E()];
for (let i = 0; i < 3; i++) es[i % 2].fire();
console.log(n);"
    );
    let pf = facts(&src);
    let v = cb_calls(&pf);
    assert!(v.iter().all(|(_, b)| *b == 0), "{v:?}\n{}", pf.dump_text());
}

/// KEEP: a callback picked from an argv-indexed array holding a builtin: no
/// bodies recorded (the set holds a builtin function value).
#[test]
fn onchg2_keep_builtin_member_records_none() {
    let src = format!(
        "{CLASS}const fs: any[] = [() => {{ n++; }}, Math.random];
class H {{ e: E; constructor() {{ const f = fs[K % 2]; const e = new E(); e.on(f); this.e = e; }} }}
const h = new H();
for (let i = 0; i < 3; i++) h.e.fire();
console.log(n);"
    );
    let pf = facts(&src);
    let v = cb_calls(&pf);
    assert!(v.iter().all(|(_, b)| *b == 0), "{v:?}\n{}", pf.dump_text());
}

/// KEEP: an unknown value (`JSON.parse` of argv) stored as the callback.
#[test]
fn onchg2_keep_unknown_value_records_none() {
    let src = format!(
        "{CLASS}class H {{ e: E; constructor() {{ const f: any = JSON.parse(String(process.argv[2])); const e = new E(); e.on(f); this.e = e; }} }}
const h = new H();
try {{ h.e.fire(); }} catch (x) {{ n = -1; }}
console.log(n);"
    );
    let pf = facts(&src);
    let v = cb_calls(&pf);
    assert!(v.iter().all(|(_, b)| *b == 0), "{v:?}\n{}", pf.dump_text());
}
