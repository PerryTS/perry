//! The canonical trie's part of the execution-prefix image: the copy
//! `js_snap_image_begin` takes, the capture roots (the backings of the nodes
//! minted or changed since), and the canonical-keys record with its stops.
//!
//! Only the image capture reaches this code: no other binary calls it.

use super::{try_with_table, CanonicalTable, NO_NODE};
use crate::snap_image::format::{self as f, CanonNode, CanonRec, Stop, Val};

/// One node as the baseline saw it: the trie column and its backing owner.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Fields {
    addr: usize,
    published: bool,
    all_ptr: bool,
    backing_slots: u32,
    run: u32,
    backing_owner: u32,
    parent: u32,
    edge_hash: u64,
    next: u32,
    len: u32,
}

impl Fields {
    /// The fields a live node changes in place (a publish, a backing, a run, a rewitness).
    fn mutable(&self) -> (usize, bool, bool, u32, u32, u32) {
        (
            self.addr,
            self.published,
            self.all_ptr,
            self.backing_slots,
            self.run,
            self.backing_owner,
        )
    }

    /// The fields only freeing a node changes (orphaning, the free-list link, reuse).
    fn freed(&self) -> (u32, u64, u32, u32) {
        (self.parent, self.edge_hash, self.next, self.len)
    }
}

/// The trie as `begin` saw it.
pub(crate) struct CanonBaseline {
    nodes: Vec<Fields>,
    free: u32,
    reaped: u64,
    /// `(parent, edge hash)` -> bucket head, sorted by key.
    heads: Vec<((u32, u64), u32)>,
}

fn fields(t: &CanonicalTable, id: usize) -> Option<Fields> {
    let n = t.nodes.get(id)?;
    if id >= t.shapes.len {
        return None;
    }
    Some(Fields {
        addr: n.addr,
        published: n.published,
        all_ptr: n.all_ptr,
        backing_slots: n.backing_slots,
        run: n.run,
        backing_owner: t.shapes[id].backing_owner,
        parent: n.parent,
        edge_hash: n.edge_hash,
        next: n.next,
        len: n.len,
    })
}

pub(super) fn baseline_of(t: &CanonicalTable) -> CanonBaseline {
    let mut nodes = Vec::with_capacity(t.nodes.len());
    for id in 0..t.nodes.len() {
        if let Some(x) = fields(t, id) {
            nodes.push(x);
        }
    }
    let mut heads: Vec<((u32, u64), u32)> = t.edges.iter().map(|(k, v)| (*k, *v)).collect();
    heads.sort_unstable();
    CanonBaseline {
        nodes,
        free: t.free,
        reaped: t.reaped,
        heads,
    }
}

/// The copy `begin` takes of this thread's trie (empty when it is gone).
pub(crate) fn snap_image_canon_baseline() -> CanonBaseline {
    try_with_table(|t| baseline_of(t)).unwrap_or(CanonBaseline {
        nodes: Vec::new(),
        free: NO_NODE,
        reaped: 0,
        heads: Vec::new(),
    })
}

/// The backing address of every node minted or changed since the baseline, and of the
/// last created list: capture roots, so each backing the record names is an image cell.
pub(super) fn words_of(t: &CanonicalTable, b: &CanonBaseline, out: &mut dyn FnMut(u64)) {
    for id in 0..t.nodes.len() {
        let Some(now) = fields(t, id) else { continue };
        if now.addr != 0 && b.nodes.get(id) != Some(&now) {
            out(now.addr as u64);
        }
    }
    if t.last_created.0 != 0 {
        out(t.last_created.0 as u64);
    }
}

pub(crate) fn snap_image_canon_words(b: &CanonBaseline, out: &mut dyn FnMut(u64)) {
    let _ = try_with_table(|t| words_of(t, b, out));
}

fn node_rec(id: usize, x: &Fields, val: &mut dyn FnMut(usize) -> Val) -> CanonNode {
    CanonNode {
        id: id as u32,
        parent: x.parent,
        edge_hash: x.edge_hash,
        len: x.len,
        all_ptr: x.all_ptr,
        published: x.published,
        addr: if x.addr == 0 { Val::Scalar(0) } else { val(x.addr) },
        backing_slots: x.backing_slots,
        run: x.run,
        backing_owner: x.backing_owner,
    }
}

/// The trie state that appeared between the baseline and the cut, and the stops of what
/// the record cannot carry: freed nodes (`STOP_CANON_FREE`), a backing that is no image
/// cell (`STOP_CANON_ADDR`), and nodes whose bucket links say they were not allocated in
/// id order (`STOP_CANON_ORDER`). `val` classifies an address as the image names it.
pub(super) fn capture_of(
    t: &CanonicalTable,
    b: &CanonBaseline,
    val: &mut dyn FnMut(usize) -> Val,
) -> (CanonRec, Vec<Stop>) {
    let begin = b.nodes.len();
    let cut = t.nodes.len();
    let mut rec = CanonRec {
        begin_len: begin as u32,
        cut_len: cut as u32,
        new: Vec::new(),
        changed: Vec::new(),
        last_addr: if t.last_created.0 == 0 {
            Val::Scalar(0)
        } else {
            val(t.last_created.0)
        },
        last_len: t.last_created.1,
        last_node: t.last_created.2,
        minted: t.minted,
        reaped: t.reaped,
    };
    // Old nodes: changed in place, or touched by a free.
    let (mut freed, mut freed_first) = (0u32, 0u32);
    for (id, was) in b.nodes.iter().enumerate() {
        let Some(now) = fields(t, id) else { continue };
        if now.freed() != was.freed() {
            if freed == 0 {
                freed_first = id as u32;
            }
            freed += 1;
        }
        if now.mutable() != was.mutable() {
            rec.changed.push(node_rec(id, &now, val));
        }
    }
    for id in begin..cut {
        if let Some(now) = fields(t, id) {
            rec.new.push(node_rec(id, &now, val));
        }
    }
    let mut stops = Vec::new();
    let stop = |kind: u8, cell: u32, off: u32, value: u64| Stop {
        cell,
        obj_type: 0,
        kind,
        off,
        value,
    };
    if t.free != b.free || t.reaped != b.reaped || freed != 0 {
        stops.push(stop(
            f::STOP_CANON_FREE,
            freed_first,
            freed,
            t.reaped.wrapping_sub(b.reaped),
        ));
    }
    let (mut bad, mut bad_first) = (0u32, None::<(u32, u64)>);
    for n in rec.new.iter().chain(rec.changed.iter()) {
        if !matches!(n.addr, Val::Scalar(0) | Val::Cell { .. } | Val::Ext { .. }) {
            bad += 1;
            if bad_first.is_none() {
                bad_first = Some((n.id, fields(t, n.id as usize).map_or(0, |x| x.addr as u64)));
            }
        }
    }
    if !matches!(rec.last_addr, Val::Scalar(0) | Val::Cell { .. } | Val::Ext { .. }) {
        bad += 1;
        if bad_first.is_none() {
            bad_first = Some((rec.last_node, t.last_created.0 as u64));
        }
    }
    if let Some((id, addr)) = bad_first {
        stops.push(stop(f::STOP_CANON_ADDR, id, bad, addr));
    }
    // Allocation in id order leaves each new node linked to the bucket head before it
    // (the previous new node of its bucket, else the baseline head), and each bucket's
    // head at the last new node in it: what `alloc_node` rebuilds in id order.
    let mut last: std::collections::BTreeMap<(u32, u64), u32> = Default::default();
    let (mut order, mut order_first) = (0u32, 0u32);
    let mut out_of_order = |id: u32| {
        if order == 0 {
            order_first = id;
        }
        order += 1;
    };
    for id in begin..cut {
        let Some(now) = fields(t, id) else { continue };
        if now.parent == NO_NODE {
            if now.next != NO_NODE {
                out_of_order(id as u32);
            }
            continue;
        }
        let key = (now.parent, now.edge_hash);
        let want = last.get(&key).copied().unwrap_or_else(|| {
            b.heads
                .binary_search_by_key(&key, |h| h.0)
                .ok()
                .and_then(|i| b.heads.get(i))
                .map_or(NO_NODE, |h| h.1)
        });
        if now.next != want {
            out_of_order(id as u32);
        }
        last.insert(key, id as u32);
    }
    for (key, id) in &last {
        if t.edges.get(key) != Some(id) {
            out_of_order(*id);
        }
    }
    if order != 0 {
        stops.push(stop(f::STOP_CANON_ORDER, order_first, order, 0));
    }
    (rec, stops)
}

/// The record and stops of this thread's trie at the cut.
pub(crate) fn snap_image_canon_capture(
    b: &CanonBaseline,
    val: &mut dyn FnMut(usize) -> Val,
) -> (CanonRec, Vec<Stop>) {
    try_with_table(|t| capture_of(t, b, val)).unwrap_or_else(|| {
        (
            CanonRec {
                begin_len: b.nodes.len() as u32,
                cut_len: 0,
                new: Vec::new(),
                changed: Vec::new(),
                last_addr: Val::Scalar(0),
                last_len: 0,
                last_node: NO_NODE,
                minted: 0,
                reaped: 0,
            },
            Vec::new(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::super::{CanonicalTable, ROOT_NODE};
    use super::*;

    fn cell(a: usize) -> Val {
        Val::Cell {
            hi: 0,
            bare: true,
            index: a as u32,
        }
    }

    fn kinds(stops: &[Stop]) -> Vec<u8> {
        stops.iter().map(|s| s.kind).collect()
    }

    #[test]
    fn snap_image_canon_record_lists_new_nodes_in_id_order_and_changed_ones() {
        let mut t = CanonicalTable::new();
        let a = t.alloc_node(0x1000, ROOT_NODE, 11, 1, true, true);
        let p = t.alloc_node(0x2000, ROOT_NODE, 12, 1, true, false);
        let b = baseline_of(&t);
        let c1 = t.alloc_node(0x3000, a, 21, 2, true, true);
        t.own_backing(c1, 4);
        let c2 = t.alloc_node(0x4000, a, 21, 2, false, true);
        let c3 = t.alloc_node(0x3000, c1, 31, 3, true, true);
        t.share_backing_owner(c3, c1);
        t.note_created(c3, c1);
        t.publish(p, 0x5000, false);
        let (rec, stops) = capture_of(&t, &b, &mut cell);
        assert!(stops.is_empty(), "{stops:?}");
        let ids: Vec<u32> = rec.new.iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![c1, c2, c3]);
        assert_eq!((rec.begin_len, rec.cut_len), (3, 6));
        let n1 = &rec.new[0];
        assert_eq!((n1.parent, n1.edge_hash, n1.len, n1.backing_slots), (a, 21, 2, 4));
        assert_eq!(n1.addr, cell(0x3000));
        let n3 = &rec.new[2];
        assert_eq!((n3.backing_owner, n3.run), (c1, 1));
        assert_eq!(rec.changed.len(), 1);
        assert_eq!((rec.changed[0].id, rec.changed[0].published), (p, true));
        assert_eq!(rec.changed[0].addr, cell(0x5000));
        assert_eq!((rec.last_addr.clone(), rec.last_len, rec.last_node), (cell(0x3000), 3, c3));
        assert_eq!(rec.minted, 5);
    }

    #[test]
    fn snap_image_canon_roots_are_the_backings_of_new_and_changed_nodes() {
        let mut t = CanonicalTable::new();
        let a = t.alloc_node(0x1000, ROOT_NODE, 11, 1, true, true);
        let p = t.alloc_node(0x2000, ROOT_NODE, 12, 1, true, false);
        let b = baseline_of(&t);
        t.alloc_node(0x3000, a, 21, 2, true, true);
        t.publish(p, 0x5000, false);
        let mut w = Vec::new();
        words_of(&t, &b, &mut |x| w.push(x));
        w.sort_unstable();
        assert_eq!(w, vec![0x3000, 0x5000]);
    }

    #[test]
    fn snap_image_canon_stops_on_a_node_freed_after_the_baseline() {
        let mut t = CanonicalTable::new();
        let a = t.alloc_node(0x1000, ROOT_NODE, 11, 1, true, true);
        let d = t.alloc_node(0x2000, ROOT_NODE, 12, 1, true, true);
        let b = baseline_of(&t);
        t.free_nodes(&[d]);
        let (_, stops) = capture_of(&t, &b, &mut cell);
        assert_eq!(kinds(&stops), vec![f::STOP_CANON_FREE], "{stops:?}");
        assert_eq!(stops[0].value, 1);
        // The freed slot is reused: same length as at the baseline, still a stop.
        t.alloc_node(0x3000, a, 21, 2, true, true);
        assert_eq!(t.nodes.len(), b.nodes.len());
        let (rec, stops) = capture_of(&t, &b, &mut cell);
        assert!(rec.new.is_empty());
        assert!(kinds(&stops).contains(&f::STOP_CANON_FREE), "{stops:?}");
    }

    #[test]
    fn snap_image_canon_stops_on_a_backing_that_is_not_an_image_cell() {
        let mut t = CanonicalTable::new();
        let b = baseline_of(&t);
        t.alloc_node(0x1000, ROOT_NODE, 11, 1, true, true);
        let bad = t.alloc_node(0x2000, ROOT_NODE, 12, 1, true, true);
        let mut val = |a: usize| if a == 0x2000 { Val::Opaque(a as u64) } else { cell(a) };
        let (_, stops) = capture_of(&t, &b, &mut val);
        assert_eq!(kinds(&stops), vec![f::STOP_CANON_ADDR], "{stops:?}");
        assert_eq!((stops[0].cell, stops[0].off, stops[0].value), (bad, 1, 0x2000));
    }

    #[test]
    fn snap_image_canon_stops_on_a_bucket_not_linked_in_id_order() {
        let mut t = CanonicalTable::new();
        let a = t.alloc_node(0x1000, ROOT_NODE, 11, 1, true, true);
        let b = baseline_of(&t);
        let c1 = t.alloc_node(0x2000, a, 21, 2, true, true);
        let c2 = t.alloc_node(0x3000, a, 21, 2, true, true);
        let (_, stops) = capture_of(&t, &b, &mut cell);
        assert!(stops.is_empty(), "{stops:?}");
        // As if c2 had been allocated before c1: the chain runs c1 -> c2.
        t.nodes[c2 as usize].next = NO_NODE;
        t.nodes[c1 as usize].next = c2;
        t.edges.insert((a, 21), c1);
        let (_, stops) = capture_of(&t, &b, &mut cell);
        assert_eq!(kinds(&stops), vec![f::STOP_CANON_ORDER], "{stops:?}");
        assert_eq!(stops[0].cell, c1);
    }
}
