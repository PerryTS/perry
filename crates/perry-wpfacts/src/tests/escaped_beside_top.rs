//! An escaped value beside ⊤ is that ⊤ (`Model::top_beside`; MERGE1 fix A,
//! 2026-11-10). Unknown code holds the value, so every rule does to ⊤ at
//! least what it does to the value: a node that holds ⊤ does not list it.
//! The arrival rule (`add_tokens_s`) covered a value that arrives at a ⊤
//! node; the solver also covers it at the two other moments: when ⊤
//! arrives at a node that lists it (A1), and when it escaped after it was
//! listed beside ⊤, at the pop that would deliver it (A2). A ⊤-free node
//! keeps its values.

use super::*;
use crate::model::{TokKind, TOP};

fn holds(m: &crate::model::Model, n: u32, t: u32) -> bool {
    m.nodes[n as usize].pts.contains_tok(t)
}

/// A1: ⊤ arrives at a node that lists a value escaped since: the value
/// leaves the node with ⊤ (and the node reads as dropped).
#[test]
fn escaped_value_listed_before_top_leaves_when_top_arrives() {
    let mut m = copy_solver_model();
    let a = m.tok(TokKind::ModNs(1));
    let b = m.tok(TokKind::ModNs(2));
    let n = m.node_l("listed");
    m.add_tok(n, a);
    m.add_tok(n, b);
    m.solve();
    assert!(holds(&m, n, a) && holds(&m, n, b));
    let r = m.reason("escaped beside top test");
    m.escape(a, r);
    m.add_tok(n, TOP);
    m.solve();
    assert!(holds(&m, n, TOP));
    assert!(!holds(&m, n, a), "escaped value still listed beside ⊤");
    assert!(holds(&m, n, b), "a value that did not escape stays");
    assert!(m.nodes[n as usize].dropped);
}

/// A2: a value listed beside ⊤ while it had not escaped, escaped before
/// the pop that delivers it: it is not delivered, and leaves the node.
#[test]
fn value_escaping_after_listed_beside_top_is_not_delivered() {
    let mut m = copy_solver_model();
    let a = m.tok(TokKind::ModNs(1));
    let b = m.tok(TokKind::ModNs(2));
    let n = m.node_l("holder");
    let d = m.node_l("reader");
    m.edge(n, d);
    m.add_tok(n, TOP);
    m.solve();
    assert!(holds(&m, d, TOP));
    m.add_tok(n, a);
    m.add_tok(n, b);
    assert!(holds(&m, n, a), "listed before it escaped");
    let r = m.reason("escaped beside top test");
    m.escape(a, r);
    m.solve();
    assert!(!holds(&m, d, a), "escaped value delivered beside ⊤");
    assert!(!holds(&m, n, a), "escaped value still listed beside ⊤");
    assert!(
        holds(&m, d, b) && holds(&m, n, b),
        "a value that did not escape is delivered"
    );
    assert!(holds(&m, d, TOP));
}

/// Control: without ⊤ an escaped value is a value like any other: listed,
/// delivered and kept.
#[test]
fn top_free_node_keeps_escaped_values() {
    let mut m = copy_solver_model();
    let a = m.tok(TokKind::ModNs(1));
    let b = m.tok(TokKind::ModNs(2));
    let n = m.node_l("holder");
    let d = m.node_l("reader");
    m.edge(n, d);
    m.add_tok(n, a);
    m.solve();
    let r = m.reason("escaped beside top test");
    m.escape(a, r);
    m.add_tok(n, b);
    m.solve();
    for x in [n, d] {
        assert!(holds(&m, x, a) && holds(&m, x, b));
        assert!(!holds(&m, x, TOP));
    }
    assert!(!m.nodes[n as usize].dropped);
}

/// Guard control (MERGE1 §3 A): an escaped instance beside ⊤ reaches an
/// `instanceof` guard. Covering it by ⊤ is sound only if the guard keeps
/// ⊤ wherever it keeps the instance: a guard that drops ⊤ but keeps listed
/// instances would lose `c` here, and `v.x` would read no receiver. The
/// read is not proven: unknown code holds `c` (it may delete `x`) and may
/// pass any object whose chain holds `C.prototype`.
#[test]
fn instanceof_guard_keeps_top_beside_escaped_instance() {
    let pf = facts(
        r#"
class C { x = 1; }
const c = new C();
const u: any = new Function(process.argv[2])(c);
const w = process.argv.length > 3;
const v: any = w ? u : c;
if (v instanceof C) {
  console.log(v.x);
}
"#,
    );
    let s = site(&pf, "x");
    // The guard keeps ⊤: the verdict is the one base 71926f2080 gives
    // (before the covering at ⊤'s arrival and at pop). A guard that kept
    // `c` but dropped ⊤ would leave the read no receiver once `c` is
    // covered, and the verdict would change.
    assert_eq!(
        format!("{:?}", s.verdict),
        r#"No("receiver from unmodeled code (TOP)")"#,
        "v.x verdict changed beside an escaped instance under an instanceof guard"
    );
    // `c` is ⊤ there: it is no longer listed beside ⊤ (base listed
    // ["t.ts:C"], an order effect of the arrival rule).
    assert!(
        s.receivers.is_empty(),
        "escaped instance listed beside ⊤: {:?}",
        s.receivers
    );
}
