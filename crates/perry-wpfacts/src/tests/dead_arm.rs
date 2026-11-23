//! DEADARM (ECMA-262 14.6.2): an `if` arm runs only when its test selects
//! it. A read of a local or of `this` in an arm, a local write and a
//! `return` pass only once the arm's test can select it and its enclosing
//! arm can run (`Gen::arm_live`, `Gen::live_gate`). RED on 85cc9b0648:
//! every arm of a live body ran, so `canvas.appendChild(image)` in
//! three.js uploadTexture's `isHTMLTexture` arm put every texture image
//! into the DOM (PERFNOW123 edge E3).
use super::*;

fn kind(pf: &ProgramFacts, key: &str) -> crate::derive::KindFact {
    let fs: Vec<_> = pf
        .templates
        .iter()
        .flat_map(|t| &t.fields)
        .filter(|f| f.key == key)
        .collect();
    assert!(!fs.is_empty(), "no field {key}");
    fs[0].kind.clone()
}

fn src(calls: &str) -> String {
    format!(
        r#"
class A {{ isA = true; img = {{ w: 1 }}; }}
class B {{ img = {{ w: 2 }}; }}
const parent = {{ kids: [] as any[] }};
const box = {{ w: 3 }};
const holder: any = {{ kid: null, other: null, nested: null, direct: null }};
function up(t: any) {{
  if (t.isA) {{
    parent.kids.push(t.img);
    holder.direct = t.img;
    if (t.img !== null) holder.nested = box;
  }} else {{
    holder.other = box;
  }}
}}
{calls}
holder.kid = parent.kids[0];
console.log(holder.kid, holder.other, holder.nested);
"#
    )
}

/// RED (PERFNOW123 E3): A is never built, so `t.isA` reads `undefined`
/// only and the arm never runs: `kids` holds nothing.
#[test]
fn deadarm_arm_whose_test_is_never_truthy_does_not_run() {
    let pf = facts(&src("up(new B());"));
    let k = kind(&pf, "kid");
    assert!(
        !k.top && k.objs.is_empty(),
        "kids holds no img: {} {:?}\n{}",
        k.desc(),
        k.objs,
        pf.dump_text()
    );
}

/// RED: a store in the dead arm stores nothing.
#[test]
fn deadarm_store_in_a_dead_arm_stores_nothing() {
    let pf = facts(&src("up(new B());"));
    let k = kind(&pf, "direct");
    assert!(
        !k.top && k.objs.is_empty(),
        "direct holds no img: {} {:?}\n{}",
        k.desc(),
        k.objs,
        pf.dump_text()
    );
}

/// RED: an `if` nested in a dead arm is dead even though its own test
/// (a compare) holds a Boolean whatever its operands are.
#[test]
fn deadarm_nested_arm_of_a_dead_arm_does_not_run() {
    let pf = facts(&src("up(new B());"));
    let k = kind(&pf, "nested");
    assert!(
        !k.top && k.objs.is_empty(),
        "nested holds no box: {} {:?}\n{}",
        k.desc(),
        k.objs,
        pf.dump_text()
    );
}

/// Guard: the else arm of a test that reads `undefined` runs.
#[test]
fn deadarm_else_of_a_falsy_test_runs() {
    let pf = facts(&src("up(new B());"));
    let k = kind(&pf, "other");
    assert!(
        k.objs.len() == 1,
        "other holds box: {}\n{}",
        k.desc(),
        pf.dump_text()
    );
}

/// Guard: once an A reaches the test, the arm runs.
#[test]
fn deadarm_arm_runs_once_its_test_can_be_truthy() {
    let pf = facts(&src("up(new B()); up(new A());"));
    let k = kind(&pf, "kid");
    assert!(
        !k.objs.is_empty(),
        "kids holds an img: {}\n{}",
        k.desc(),
        pf.dump_text()
    );
    let n = kind(&pf, "nested");
    assert!(
        n.objs.len() == 1,
        "nested holds box: {}\n{}",
        n.desc(),
        pf.dump_text()
    );
}

/// Guard: a ⊤ test (a parsed input) runs both arms.
#[test]
fn deadarm_top_test_runs_both_arms() {
    let pf = facts(
        r#"
const box = { w: 1 };
const h: any = { a: null, b: null };
const x: any = JSON.parse(process.argv[2]);
if (x) { h.a = box; } else { h.b = box; }
console.log(h.a, h.b);
"#,
    );
    for f in ["a", "b"] {
        let k = kind(&pf, f);
        assert!(
            k.objs.len() == 1,
            "{f} holds box: {}\n{}",
            k.desc(),
            pf.dump_text()
        );
    }
}

/// Guard: a test that turns truthy only late in the fixpoint (a store a
/// callback makes once the call through the array resolves) still opens
/// the arm, and a local written there reaches its later read.
#[test]
fn deadarm_test_truthy_late_in_the_fixpoint_runs() {
    let pf = facts(
        r#"
const st: any = { on: undefined };
const box = { w: 1 };
const h: any = { v: null, r: null };
function chk(): any {
  let got: any = null;
  if (st.on) { got = box; return got; }
  return null;
}
const fs: any[] = [];
fs.push(() => { st.on = 1; });
for (const f of fs) f();
h.v = chk();
console.log(h.v);
"#,
    );
    let k = kind(&pf, "v");
    assert!(
        k.objs.len() == 1,
        "v holds box: {}\n{}",
        k.desc(),
        pf.dump_text()
    );
}

/// PERFNOW131: a function whose body has a gated arm keeps its summary
/// and its instances. RED before the fix: the arm's `live` node joined
/// `dispatch`'s slice through its test, and with it every read the arm
/// gates (a guard's `from`), so the narrowed element read `t` (its
/// `elemundef` rule, a solver rule) refused the summary: one shared body,
/// and the receiver of every call merged (perry-web events.ts:375
/// `dispatch`, globalThis reaching node.ts:126 `n._parent`).
#[test]
fn deadarm_gated_body_keeps_its_summary_instances() {
    // `if (event.bubbles)` is a parameter-tested arm whose body reads the
    // local `path` and narrows an element of it (`t !== undefined`, an
    // elemundef rule). The arm gate is control, not data: the gated read
    // of `path` stays as shared as without DEADARM, and `dispatch` keeps
    // its summary instances.
    let src = r#"
class T {
  name: string; up: T | null;
  constructor(n: string, u: T | null) { this.name = n; this.up = u; }
  self(): T { return this; }
  parent(e: Ev): T | null { return this.up; }
  fire(e: Ev): boolean { return dispatch(this, e); }
}
class Ev { phase = 0; stop = false; bubbles = true; target: any = null; path: T[] = []; }
function invoke(t: T, e: Ev, ph: number, c: boolean): void { if (t.name === "x") e.stop = true; }
function dispatch(target: T, event: Ev): boolean {
  if (event.phase !== 0) throw new Error("busy");
  event.target = target.self();
  const path: T[] = [target];
  for (let p = target.parent(event); p !== null; p = p.parent(event)) path.push(p);
  event.path = path.map((t) => t.self());
  for (let i = path.length - 1; i > 0 && !event.stop; i--) { const t = path[i]; if (t !== undefined) invoke(t, event, 1, true); }
  if (!event.stop) invoke(target, event, 2, true);
  if (event.bubbles) for (let i = 1; i < path.length && !event.stop; i++) { const t = path[i]; if (t !== undefined) invoke(t, event, 3, false); }
  event.phase = 0;
  return !event.stop;
}
const a = new T("a", null);
const b = new T("b", a);
a.fire(new Ev());
b.fire(new Ev());
dispatch(b, new Ev());
"#;
    let ast = perry_parser::parse_typescript(src, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let (g, _, _) = crate::generate(
        &[("t.ts", &hir)],
        &FactsConfig::default(),
        None,
        None,
        None,
        crate::Hold::Midway,
        std::time::Instant::now(),
    );
    let d =
        g.m.funcs
            .iter()
            .position(|f| f.name.ends_with(":dispatch"))
            .unwrap() as u32;
    assert!(
        g.m.sum.of.contains_key(&d),
        "dispatch must keep its summary: {}",
        g.m.sum.stats
    );
}
