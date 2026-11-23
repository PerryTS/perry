//! ET1 (PERFNOW27E): `new EventTarget()` runs the realm declaration of the
//! runtime constructor (`js_event_target_new`: a fresh object whose own
//! listener state is stored on it, linked to `EventTarget.prototype`; no
//! argument, no global, no call). Before, the declaration had no effects:
//! the call was one of an unknown constructor and held the realm, so the
//! intrinsic iterator `next` of every program `for..of` counted as written.
//! RED: the pair (repro of pn26e) with the statement holds no realm and
//! proves the same sites as without it.
use super::*;

const PAIR: &str = r#"
function EventTargetBase() {
}
class ET extends EventTargetBase() {}
interface Init {
}
const sinks: ((record: Init) => void)[] = [];
function mutated(record: Init): void { for (const sink of sinks) sink(record); }
class Nd extends ET {
  _children: Nd[] = [];
  _replaceAll(nodes: Nd[]): void {
    const old = this._children.slice();
    if (old.length > 0 || nodes.length > 0) mutated({ type: "childList", target: this, addedNodes: nodes, removedNodes: old });
  }
}
class Doc extends Nd {
}
const observers = new Set<Obs>();
class Obs {
  _offer(r: Init): void {
  }
}
sinks.push((r) => {
  for (const o of observers) o._offer(r);
});
function parseDocument(doc: Doc, markup: string): void {
  doc._replaceAll([]);
}
const document = new Doc();
parseDocument(document, "<html></html>");
"#;

fn held(src: &str) -> bool {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m.realm_held
}

fn verdicts(src: &str) -> Vec<(String, usize, String)> {
    let pf = facts(src);
    let mut v: Vec<_> = pf
        .sites
        .iter()
        .map(|s| {
            (
                s.key.to_string(),
                s.byte_offset as usize,
                format!("{:?}", s.verdict),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn et1_new_event_target_runs_its_declaration() {
    let bad = format!("{PAIR}new EventTarget();\n");
    assert!(!held(PAIR), "control: realm held without EventTarget");
    assert!(!held(&bad), "new EventTarget() holds the realm");
    assert_eq!(verdicts(&bad), verdicts(PAIR));
}
