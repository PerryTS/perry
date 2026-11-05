//! Equal points-to sets are stored once (SCALE1 R2): nodes that reach the same
//! content on different paths share one stored set, and the facts stay as
//! they were.
use crate::model::TokKind;

#[test]
fn equal_sets_reached_on_different_paths_are_stored_once() {
    let mut m = super::copy_solver_model();
    let toks: Vec<_> = (0..60).map(|i| m.tok(TokKind::ModNs(i))).collect();
    let mut holders = Vec::new();
    for k in 0..100usize {
        let src = m.node_l("seeded source");
        // Relays registered before the source holds anything get its values
        // by delivery, relays registered after by registration.
        let early = m.node_l("early relay");
        m.edge(src, early);
        // Every source receives the same members in its own order.
        let mut order = toks.clone();
        order.rotate_left(k % toks.len());
        for &t in &order {
            m.add_tok(src, t);
        }
        let late = m.node_l("late relay");
        m.edge(src, late);
        holders.extend([src, early, late]);
    }
    m.solve();
    assert!(!m.aborted);
    for &n in &holders {
        let pts = &m.nodes[n as usize].pts;
        assert_eq!(pts.len(), toks.len());
        assert_eq!(pts.to_sorted(), toks);
    }
    let (logical, stored, distinct) = m.pts_storage();
    // 300 holders of 60 members, plus the unmodeled node's TOP.
    assert_eq!(logical, 300 * 60 + 1);
    assert_eq!(
        (stored, distinct),
        (60 + 1, 1),
        "300 equal sets are stored {distinct} times ({stored} stored entries)"
    );
}
