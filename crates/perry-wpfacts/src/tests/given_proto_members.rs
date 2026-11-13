//! Rule GP1: a program object `P` given as the [[Prototype]] of a built-in
//! object `B` (`__proto__`, `Object.setPrototypeOf`; ECMA-262 B.2.2.1.2,
//! 10.1.2) is not read or called by the give. A later [[Get]] on `B` that
//! misses `B`'s own keys continues at `P` (10.1.8.1): a program read of `B`
//! reads `P`'s chain exactly, and a member of `P`'s chain reaches code the
//! model does not follow only under a key the runtime's own protocol
//! lookups read. So `P`'s other members do not escape.
use super::*;

const M: &str = r#"
class M {
  inRun: number;
  constructor() { this.inRun = 1; }
  run() { return this.inRun; }
}
"#;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    !matches!(site(pf, key).verdict, Verdict::No(_))
}

/// Does the solve reach the body of function `name` (a call the model
/// follows, or a function handed to code it does not follow)?
fn runs(src: &str, name: &str) -> (bool, ProgramFacts) {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = hir.functions.iter().find(|f| f.name == name).expect("fn").id;
    let hit = pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id);
    (hit, pf)
}

/// (a) `b` is what an unknown string key reads on `{}` (an
/// `Object.prototype` member may be one); an unknown string key store into
/// it may be a `__proto__` store.
#[test]
fn a_given_prototype_keeps_its_methods() {
    let pf = facts(&format!(
        r#"{M}
const k = String(Math.random());
const k2 = String(Math.random());
const b: any = ({{}} as any)[k];
const m = new M();
if (b !== undefined) b[k2] = m;
console.log(m.run());
"#
    ));
    assert!(proven(&pf, "inRun"), "{}", pf.dump_text());
    assert!(proven(&pf, "run"), "{}", pf.dump_text());
}

/// (b) control: a member under a protocol key the runtime reads on `B`
/// (`toString`) is still handed to the runtime.
#[test]
fn a_protocol_member_of_a_given_prototype_escapes() {
    let (hit, pf) = runs(
        r#"
function hit() { return 1; }
class T {
  inStr: number;
  constructor() { this.inStr = 1; }
  toString() { hit(); return "t"; }
}
const k = String(Math.random());
const k2 = String(Math.random());
const b: any = ({} as any)[k];
const t = new T();
if (b !== undefined) b[k2] = t;
console.log(t.inStr);
"#,
        "hit",
    );
    assert!(hit, "{}", pf.dump_text());
}

/// (c) control: a program read on the built-in continues into the given
/// chain: `Object.run` is M.prototype.run.
#[test]
fn a_program_read_on_the_builtin_reads_the_given_chain() {
    let (hit, pf) = runs(
        r#"
function hit() { return 1; }
class M { run() { return hit(); } }
Object.setPrototypeOf(Object, new M());
console.log((Object as any).run());
"#,
        "hit",
    );
    assert!(hit, "{}", pf.dump_text());
    assert!(tmpl(&pf, "M").escape.is_none(), "{}", pf.dump_text());
}

/// (d) control: an own member of the built-in stays the built-in.
#[test]
fn an_own_member_of_the_builtin_stays() {
    let pf = facts(&format!(
        r#"{M}
Object.setPrototypeOf(Object, new M());
const o: any = Object;
console.log(o.keys({{ a: 1 }}).length);
"#
    ));
    assert!(proven(&pf, "length"), "{}", pf.dump_text());
}

/// (e) control: an unknown `B` (⊤) keeps today's conservative result.
#[test]
fn an_unknown_receiver_hands_the_value_on() {
    let pf = facts(&format!(
        r#"{M}
const k2 = String(Math.random());
const b: any = (globalThis as any).hostFn();
const m = new M();
b[k2] = m;
console.log(m.run());
"#
    ));
    assert!(!proven(&pf, "inRun"), "{}", pf.dump_text());
}

/// (f) three.js AnimationMixer._addInactiveBinding (three.core.js:55571-55580).
#[test]
fn the_binding_by_name_store_keeps_its_methods() {
    let pf = facts(&format!(
        r#"{M}
const byRoot: any = {{}};
function bind(uuid: string, trackName: string): M {{
  let byName = byRoot[uuid];
  if (byName === undefined) {{ byName = {{}}; byRoot[uuid] = byName; }}
  const m = new M();
  byName[trackName] = m;
  return m;
}}
const m = bind(String(Math.random()), String(Math.random()));
console.log(m.run());
"#
    ));
    assert!(proven(&pf, "inRun"), "{}", pf.dump_text());
    assert!(proven(&pf, "run"), "{}", pf.dump_text());
}
