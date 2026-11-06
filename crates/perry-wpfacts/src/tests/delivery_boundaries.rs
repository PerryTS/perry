//! Saved solver dumps cover every node, not just proven sites.
use crate::model::{prim, Model, TokKind};
use std::cell::RefCell;
thread_local! {
    static DUMPS: RefCell<Option<String>> = const { RefCell::new(None) };
}
pub(crate) fn record(m: &Model) {
    DUMPS.with_borrow_mut(|out| {
        if let Some(out) = out {
            out.push_str(&crate::dump::solver_state(m));
        }
    });
}
const PROGRAMS: &[&str] = &[
    r#"class A { x = 1; } class B { y = 's'; }
    const a: any = new A(); const b: any = new B();
    const key = (globalThis as any).key; a[key] = b;
    function get(o: any) { return o.x; }
    console.log(get(a), get(b), a[key]);"#,
    r#"class A { x = 1; } class G { get value() { return new A(); } }
    function read(o: any) { return o.value; }
    const a = read(new G()); console.log(a.x);
    function dead() { return new G(); }"#,
    r#"const p: any = { x: { a: 1 } }; const q: any = { x: { b: 2 } };
    const o: any = Object.create(p); const before = o.x;
    Object.setPrototypeOf(o, q); console.log(before.a, o.x.b);"#,
    r#"class A { x = 1; } class B { y = 2; }
    function id(o: any) { return o; }
    function pass(o: any) { const p = o; return id(p); }
    let a: any = pass(new A()); let b: any = pass(new B());
    for (let i=0;i<2;i++) { const c = a; a = b; b = c; }
    console.log(a.x, b.y);"#,
];
#[test]
fn bounded_programs_match_full_base_solver_dump() {
    DUMPS.with_borrow_mut(|v| *v = Some(String::new()));
    for src in PROGRAMS {
        let pf = super::facts(src);
        assert!(!pf.stats.aborted);
    }
    let dump = DUMPS.with_borrow_mut(|v| v.take().unwrap());
    assert_eq!(dump, include_str!("fixtures/delivery-base.dump"));
}
#[test]
fn pending_event_across_pruning_uses_retained_prefix_rank() {
    let mut m = super::copy_solver_model();
    let src = m.node_l("source");
    let dst: Vec<_> = (0..4).map(|_| m.node_l("receiver")).collect();
    m.edge(src, dst[0]);
    m.edge(src, dst[1]);
    let t = m.tok(TokKind::ModNs(1));
    m.add_tok(src, t);
    m.edge(src, dst[2]);
    m.edge(src, dst[3]);
    m.prune_successors(src, &[false, true, true, true]);
    let pending = m.pending.snapshot(&m.nodes[src as usize].delta);
    let deliveries: Vec<_> = m.nodes[src as usize]
        .succ
        .iter()
        .enumerate()
        .map(|(i, _)| pending.delivery_len(i as u32))
        .collect();
    assert_eq!(deliveries, [1, 0, 0]);
    m.solve();
    assert!(!m.aborted);
    assert!(m.nodes[dst[0] as usize].pts.is_empty());
    for &n in &dst[1..] {
        assert!(m.nodes[n as usize].pts.contains_tok(t));
    }
}
#[test]
fn late_successor_receives_each_earlier_value_once() {
    let mut m = super::copy_solver_model();
    let src = m.node_l("source");
    let early = m.node_l("early");
    let late = m.node_l("late");
    let a = m.tok(TokKind::ModNs(1));
    let b = m.tok(TokKind::ModNs(2));
    m.edge(src, early);
    m.add(src, &[a, b], prim::STR);
    m.edge(src, late);
    assert_eq!(
        m.pending
            .snapshot(&m.nodes[late as usize].delta)
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [a, b]
    );
    assert_eq!(
        m.pending
            .snapshot(&m.nodes[src as usize].delta)
            .delivery_len(1),
        0
    );
    assert_eq!(
        m.pending
            .snapshot(&m.nodes[src as usize].delta)
            .delivery_len(0),
        2
    );
    m.edge(src, late);
    assert_eq!(m.nodes[late as usize].delta.len(), 2);
    m.solve();
    assert!(!m.aborted);
    assert_eq!(m.nodes[late as usize].pts.to_sorted(), [a, b]);
    assert_eq!(m.nodes[late as usize].prim, prim::STR);
}
#[test]
fn drained_event_storage_is_pending_only() {
    let mut m = super::copy_solver_model();
    let t = m.tok(TokKind::ModNs(1));
    for _ in 0..1000 {
        let src = m.node_l("source");
        let dst = m.node_l("receiver");
        m.edge(src, dst);
        m.add_tok(src, t);
    }
    m.solve();
    assert!(!m.aborted);
    assert!(m.work.get() >= 1000);
    assert_eq!(
        m.nodes.iter().filter(|n| n.pts.contains_tok(t)).count(),
        2000
    );
    assert!(m.nodes.iter().all(|n| n.delta.is_empty()));
    let bytes = m.pending.retained_bytes();
    assert!(bytes < 4096, "drained event storage retains {bytes} bytes");
}

#[test]
fn interleaved_pages_preserve_live_chains_and_release_empty_pages() {
    use crate::delta::{Delta, EventQueue, Pending};
    let mut queue = EventQueue::default();
    let mut nodes = vec![Pending::default(); 1000];
    for (i, n) in nodes.iter_mut().enumerate() {
        queue.push_to(n, i as u32, 1);
        queue.push_to(n, 1000 + i as u32, 2);
    }
    let mut delta = Delta::default();
    for i in (0..1000).step_by(2) {
        queue.take(&mut nodes[i], &mut delta);
        assert_eq!(
            delta.iter().copied().collect::<Vec<_>>(),
            [i as u32, 1000 + i as u32]
        );
        delta.clear();
    }
    for i in (0..1000).step_by(2) {
        queue.push_to(&mut nodes[i], 2000 + i as u32, 3);
    }
    for i in (0..1000).rev() {
        queue.take(&mut nodes[i], &mut delta);
        let expected = if i % 2 == 0 {
            vec![2000 + i as u32]
        } else {
            vec![i as u32, 1000 + i as u32]
        };
        assert_eq!(delta.iter().copied().collect::<Vec<_>>(), expected);
        delta.clear();
    }
    assert_eq!(queue.retained_bytes(), 0);
    assert_eq!(std::mem::size_of::<Pending>(), 16);
}
#[test]
fn duplicate_registration_preserves_pending_batch_work() {
    for count in [16, 17, 20] {
        let mut m = super::copy_solver_model();
        let src = m.node_l("source");
        let receivers: Vec<_> = (0..count).map(|_| m.node_l("receiver")).collect();
        for &dst in &receivers {
            m.edge(src, dst);
        }
        let a = m.tok(TokKind::ModNs(1));
        let b = m.tok(TokKind::ModNs(2));
        m.add_tok(src, a);
        m.edge(src, receivers[0]);
        m.add_tok(src, b);
        let pending = m.pending.snapshot(&m.nodes[src as usize].delta);
        assert_eq!(
            pending.delivery_overhead(),
            u64::from(count > 16),
            "{count} receivers"
        );
        m.solve();
        assert!(!m.aborted);
        for &dst in &receivers {
            assert_eq!(m.nodes[dst as usize].pts.to_sorted(), [a, b]);
        }
    }
}
#[test]
fn small_pending_capture_still_merges_after_pruning_later_edges() {
    let mut m = super::copy_solver_model();
    let src = m.node_l("source");
    let receivers: Vec<_> = (0..17).map(|_| m.node_l("receiver")).collect();
    for &dst in &receivers[..16] {
        m.edge(src, dst);
    }
    let a = m.tok(TokKind::ModNs(1));
    let b = m.tok(TokKind::ModNs(2));
    m.add_tok(src, a);
    m.edge(src, receivers[16]);
    m.edge(src, receivers[16]);
    let mut keep = vec![true; 17];
    keep[16] = false;
    m.prune_successors(src, &keep);
    m.add_tok(src, b);
    let pending = m.pending.snapshot(&m.nodes[src as usize].delta);
    assert_eq!(pending.delivery_overhead(), 0);
    m.solve();
    assert!(!m.aborted);
    for &dst in &receivers[..16] {
        assert_eq!(m.nodes[dst as usize].pts.to_sorted(), [a, b]);
    }
    assert_eq!(m.nodes[receivers[16] as usize].pts.to_sorted(), [a]);
}
