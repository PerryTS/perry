//! The canonical trie's part of the execution-prefix image: the copy
//! `js_snap_image_begin` takes, the capture roots (the backings of the nodes
//! minted or changed since), the canonical-keys record with its stops, and its replay.
//!
//! Only the image capture reaches this code: no other binary calls it.

use super::{try_with_table, CanonicalTable, Node, NodeShapes, NO_NODE};
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

impl CanonBaseline {
    /// A copy the replay thread owns: the replay runs on a thread of its own, which cannot
    /// borrow the baseline the capture holds.
    pub(crate) fn copy(&self) -> CanonBaseline {
        CanonBaseline {
            nodes: self.nodes.clone(),
            free: self.free,
            reaped: self.reaped,
            heads: self.heads.clone(),
        }
    }
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
        addr: if x.addr == 0 {
            Val::Scalar(0)
        } else {
            val(x.addr)
        },
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
    if !matches!(
        rec.last_addr,
        Val::Scalar(0) | Val::Cell { .. } | Val::Ext { .. }
    ) {
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

/// The record and stops of this thread's trie at the cut, or `None` once the trie is torn down.
pub(crate) fn snap_image_canon_capture(
    b: &CanonBaseline,
    val: &mut dyn FnMut(usize) -> Val,
) -> Option<(CanonRec, Vec<Stop>)> {
    try_with_table(|t| capture_of(t, b, val))
}

/// Why a replay cannot rebuild the trie the record describes: the node, and what of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonReplayError {
    pub(crate) node: u32,
    pub(crate) what: &'static str,
}

fn replay_error(node: u32, what: &'static str) -> CanonReplayError {
    CanonReplayError { node, what }
}

/// Node `id`'s shape entry, or `None` past the column: checked access, so the replay and
/// its check carry no panic site.
fn shape_mut(t: &mut CanonicalTable, id: usize) -> Option<&mut NodeShapes> {
    // SAFETY: `id` is below the initialized length.
    (id < t.shapes.len).then(|| unsafe { t.shapes.slot_mut(id) })
}

fn shape_of(t: &CanonicalTable, id: usize) -> Option<&NodeShapes> {
    // SAFETY: `id` is below the initialized length.
    (id < t.shapes.len).then(|| unsafe { t.shapes.slot(id) })
}

/// Replays the record into `t`, a trie as the baseline left it: each new node is allocated
/// in id order through the node allocator (which rebuilds the bucket chains, since it
/// prepends in the same order) and must come back under its recorded id; then its backing
/// column, its run and backing owner are set; then each changed node gets its mutable
/// fields, then the table scalars. `addr` turns a recorded backing into its address.
pub(super) fn replay_into(
    t: &mut CanonicalTable,
    rec: &CanonRec,
    addr: &mut dyn FnMut(&Val) -> usize,
) -> Result<(), CanonReplayError> {
    if t.nodes.len() != rec.begin_len as usize {
        return Err(replay_error(
            t.nodes.len() as u32,
            "the trie is not at the baseline length",
        ));
    }
    for n in &rec.new {
        let id = t.alloc_node(
            addr(&n.addr),
            n.parent,
            n.edge_hash,
            n.len,
            n.all_ptr,
            n.published,
        );
        if id != n.id {
            return Err(replay_error(n.id, "the node allocator returned another id"));
        }
        let Some(node) = t.nodes.get_mut(id as usize) else {
            return Err(replay_error(n.id, "the node allocator returned no node"));
        };
        node.backing_slots = n.backing_slots;
        node.run = n.run;
        let Some(shape) = shape_mut(t, id as usize) else {
            return Err(replay_error(n.id, "the node allocator left no shape entry"));
        };
        shape.backing_owner = n.backing_owner;
    }
    for n in &rec.changed {
        let Some(node) = t
            .nodes
            .get_mut(n.id as usize)
            .filter(|_| n.id < rec.begin_len)
        else {
            return Err(replay_error(n.id, "a changed node is not a baseline node"));
        };
        if (node.parent, node.edge_hash, node.len) != (n.parent, n.edge_hash, n.len) {
            return Err(replay_error(
                n.id,
                "a changed node has another parent, edge or length",
            ));
        }
        node.addr = addr(&n.addr);
        node.published = n.published;
        node.all_ptr = n.all_ptr;
        node.backing_slots = n.backing_slots;
        node.run = n.run;
        let Some(shape) = shape_mut(t, n.id as usize) else {
            return Err(replay_error(n.id, "a changed node has no shape entry"));
        };
        shape.backing_owner = n.backing_owner;
    }
    t.last_created = (addr(&rec.last_addr), rec.last_len, rec.last_node);
    t.minted = rec.minted;
    t.reaped = rec.reaped;
    if t.nodes.len() != rec.cut_len as usize {
        return Err(replay_error(
            t.nodes.len() as u32,
            "the trie is not at the cut length",
        ));
    }
    Ok(())
}

/// Restore (step 3a, before the shape replay): replays the record into this thread's trie,
/// which holds the baseline. A record the trie cannot take, or a torn-down trie, is a broken
/// image: the error names the node and the reason, and the restore start-up refuses the
/// image with it.
// The restore calls it from the image start-up; until then only the tests do.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn snap_image_canon_replay(
    rec: &CanonRec,
    addr: &mut dyn FnMut(&Val) -> usize,
) -> Result<(), CanonReplayError> {
    try_with_table(|t| replay_into(t, rec, addr))
        .unwrap_or(Err(replay_error(NO_NODE, "the trie is torn down")))
}

/// Sets `t` to exactly the baseline's nodes, bucket heads, free list and reaped count, in
/// place: the table keeps its own allocations, so no other table is built or dropped.
pub(super) fn reset_to(t: &mut CanonicalTable, b: &CanonBaseline) {
    t.nodes.clear();
    for (id, x) in b.nodes.iter().enumerate() {
        t.nodes.push(Node {
            addr: x.addr,
            published: x.published,
            parent: x.parent,
            edge_hash: x.edge_hash,
            next: x.next,
            len: x.len,
            all_ptr: x.all_ptr,
            backing_slots: x.backing_slots,
            run: x.run,
        });
        if id >= t.shapes.len {
            t.shapes.push(NodeShapes::default());
        }
        if let Some(shape) = shape_mut(t, id) {
            shape.backing_owner = x.backing_owner;
        }
    }
    t.edges.clear();
    for (k, v) in &b.heads {
        t.edges.insert(*k, *v);
    }
    t.free = b.free;
    t.reaped = b.reaped;
}

/// A trie as plain values: every node's fields with its backing owner, the bucket heads
/// sorted, and the free list and scalars.
#[derive(PartialEq, Eq)]
pub(crate) struct TrieView {
    nodes: Vec<(usize, bool, u32, u64, u32, u32, bool, u32, u32, u32)>,
    heads: Vec<((u32, u64), u32)>,
    scalars: (u32, (usize, u32, u32), u64, u64),
}

pub(super) fn view_of(t: &CanonicalTable) -> TrieView {
    let mut nodes = Vec::with_capacity(t.nodes.len());
    for (id, n) in t.nodes.iter().enumerate() {
        let owner = shape_of(t, id).map_or(NO_NODE, |x| x.backing_owner);
        nodes.push((
            n.addr,
            n.published,
            n.parent,
            n.edge_hash,
            n.next,
            n.len,
            n.all_ptr,
            n.backing_slots,
            n.run,
            owner,
        ));
    }
    let mut heads: Vec<((u32, u64), u32)> = t.edges.iter().map(|(k, v)| (*k, *v)).collect();
    heads.sort_unstable();
    TrieView {
        nodes,
        heads,
        scalars: (t.free, t.last_created, t.minted, t.reaped),
    }
}

/// The first node (or `NO_NODE` for a table field) where two tries differ.
pub(super) fn first_difference(a: &TrieView, b: &TrieView) -> Option<CanonReplayError> {
    for id in 0..a.nodes.len().max(b.nodes.len()) {
        if a.nodes.get(id) != b.nodes.get(id) {
            return Some(replay_error(
                id as u32,
                "a node field or backing owner differs",
            ));
        }
    }
    if a.heads != b.heads {
        return Some(replay_error(NO_NODE, "the bucket heads differ"));
    }
    if a.scalars != b.scalars {
        return Some(replay_error(
            NO_NODE,
            "the free list or a table scalar differs",
        ));
    }
    None
}

/// The first half of the capture's check of the replay: the record as the image file holds
/// it, replayed into this thread's trie reset to the baseline. It overwrites the calling
/// thread's trie, so the capture calls it on a thread of its own (whose trie is fresh); that
/// way the capture builds no second table. `addr` turns a recorded backing back into the
/// address the capture classified. `None` once the trie is torn down.
pub(crate) fn snap_image_canon_replay_apart(
    b: &CanonBaseline,
    rec: &CanonRec,
    addr: &dyn Fn(&Val) -> usize,
) -> Option<Result<TrieView, CanonReplayError>> {
    try_with_table(|t| {
        reset_to(t, b);
        replay_into(t, rec, &mut |v| addr(v))?;
        Ok(view_of(t))
    })
}

/// The second half: the replayed trie must be this thread's trie at the cut.
pub(crate) fn snap_image_canon_check(
    replayed: Result<TrieView, CanonReplayError>,
) -> Option<Result<(), CanonReplayError>> {
    try_with_table(|live| match replayed {
        Ok(v) => first_difference(&v, &view_of(live)).map_or(Ok(()), Err),
        Err(e) => Err(e),
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
        assert_eq!(
            (n1.parent, n1.edge_hash, n1.len, n1.backing_slots),
            (a, 21, 2, 4)
        );
        assert_eq!(n1.addr, cell(0x3000));
        let n3 = &rec.new[2];
        assert_eq!((n3.backing_owner, n3.run), (c1, 1));
        assert_eq!(rec.changed.len(), 1);
        assert_eq!((rec.changed[0].id, rec.changed[0].published), (p, true));
        assert_eq!(rec.changed[0].addr, cell(0x5000));
        assert_eq!(
            (rec.last_addr.clone(), rec.last_len, rec.last_node),
            (cell(0x3000), 3, c3)
        );
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
        let mut val = |a: usize| {
            if a == 0x2000 {
                Val::Opaque(a as u64)
            } else {
                cell(a)
            }
        };
        let (_, stops) = capture_of(&t, &b, &mut val);
        assert_eq!(kinds(&stops), vec![f::STOP_CANON_ADDR], "{stops:?}");
        assert_eq!(
            (stops[0].cell, stops[0].off, stops[0].value),
            (bad, 1, 0x2000)
        );
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

    /// The record's backings are test addresses: `cell` names each by its address.
    fn uncell(v: &Val) -> usize {
        match v {
            Val::Cell { index, .. } => *index as usize,
            _ => 0,
        }
    }

    /// A fresh trie reset to the baseline (the tests' own table; the capture resets its
    /// check thread's trie the same way).
    fn baseline_table(b: &CanonBaseline) -> CanonicalTable {
        let mut t = CanonicalTable::new();
        t.alloc_node(0xdead, ROOT_NODE, 1, 1, true, true);
        reset_to(&mut t, b);
        t
    }

    /// A baseline, then three lists grown (one forking off another, one sharing its parent's
    /// backing) and a baseline node published in place.
    fn grown() -> (CanonicalTable, CanonBaseline) {
        let mut t = CanonicalTable::new();
        let a = t.alloc_node(0x1000, ROOT_NODE, 11, 1, true, true);
        t.own_backing(a, 4);
        t.note_created(a, ROOT_NODE);
        let p = t.alloc_node(0x1000, a, 12, 2, true, false);
        let b = baseline_of(&t);
        // A baseline node reached since: its run is reset in place.
        t.note_reached(a);
        // List 1: a -> c1 -> c2, on a fresh backing then tip appends on it.
        let c1 = t.alloc_node(0x2000, a, 21, 2, true, true);
        t.own_backing(c1, 6);
        t.note_created(c1, a);
        let c2 = t.alloc_node(0x2000, c1, 31, 3, false, true);
        t.share_backing_owner(c2, c1);
        t.note_created(c2, c1);
        // List 2 forks off list 1 at c1, in the bucket of c2's edge (same parent and hash).
        let d2 = t.alloc_node(0x3000, c1, 31, 3, true, true);
        t.own_backing(d2, 3);
        t.note_created(d2, c1);
        // List 3: a new root edge, two keys.
        let e1 = t.alloc_node(0x4000, ROOT_NODE, 13, 1, true, true);
        t.own_backing(e1, 2);
        let e2 = t.alloc_node(0x4000, e1, 41, 2, true, true);
        t.share_backing_owner(e2, e1);
        t.note_created(e2, e1);
        // The baseline prefix p is published on a backing of its own.
        t.publish(p, 0x5000, true);
        t.own_backing(p, 2);
        t.note_reached(c1);
        (t, b)
    }

    #[test]
    fn snap_image_canon_replay_rebuilds_the_trie_at_the_cut() {
        let (t, b) = grown();
        let (rec, stops) = capture_of(&t, &b, &mut cell);
        assert!(stops.is_empty(), "{stops:?}");
        assert_eq!((rec.new.len(), rec.changed.len()), (5, 2));
        // The record as the image file holds it.
        let blob = f::canon_blob(&rec);
        let (back, _) = f::parse_canon_blob(&blob).expect("the record reads back");
        let mut r = baseline_table(&b);
        assert_eq!(
            first_difference(&view_of(&r), &view_of(&baseline_table(&b))),
            None
        );
        assert_eq!(replay_into(&mut r, &back, &mut uncell), Ok(()));
        assert_eq!(first_difference(&view_of(&r), &view_of(&t)), None);
        // Every bucket chain, walked from its head, lists the same nodes in the same order.
        let chain = |x: &CanonicalTable, mut id: u32| {
            let mut v = Vec::new();
            while id != NO_NODE {
                v.push(id);
                id = x.nodes[id as usize].next;
            }
            v
        };
        let mut heads: Vec<_> = t.edges.iter().map(|(k, v)| (*k, *v)).collect();
        heads.sort_unstable();
        assert_eq!(heads.len(), r.edges.len());
        for (k, h) in heads {
            assert_eq!(chain(&r, r.edges[&k]), chain(&t, h), "bucket {k:?}");
        }
    }

    #[test]
    fn snap_image_canon_replay_names_the_node_whose_id_differs() {
        let (t, b) = grown();
        let (mut rec, _) = capture_of(&t, &b, &mut cell);
        rec.new.swap(0, 1);
        let mut r = baseline_table(&b);
        let e = replay_into(&mut r, &rec, &mut uncell).expect_err("ids out of order");
        assert_eq!(e.node, rec.new[0].id);
        // A trie that is not at the baseline length cannot take the record either.
        let (rec, _) = capture_of(&t, &b, &mut cell);
        let mut r = baseline_table(&b);
        r.alloc_node(0x9000, ROOT_NODE, 99, 1, true, true);
        assert!(replay_into(&mut r, &rec, &mut uncell).is_err());
    }

    #[test]
    fn snap_image_canon_check_finds_a_changed_node_left_out() {
        let (t, b) = grown();
        let (mut rec, _) = capture_of(&t, &b, &mut cell);
        let p = rec
            .changed
            .iter()
            .position(|n| n.published && n.addr == cell(0x5000))
            .expect("p changed");
        let gone = rec.changed.remove(p);
        let mut r = baseline_table(&b);
        assert_eq!(replay_into(&mut r, &rec, &mut uncell), Ok(()));
        assert_eq!(
            first_difference(&view_of(&r), &view_of(&t)).map(|e| e.node),
            Some(gone.id)
        );
    }

    #[test]
    fn snap_image_canon_replay_takes_the_record_into_the_thread_trie() {
        let (t, b) = grown();
        let (rec, _) = capture_of(&t, &b, &mut cell);
        let want = view_of(&t);
        // A thread of its own: its trie is fresh, and the test thread's stays untouched.
        let got = std::thread::spawn(move || {
            try_with_table(|x| reset_to(x, &b)).expect("trie");
            snap_image_canon_replay(&rec, &mut uncell).expect("the replay takes the record");
            try_with_table(|x| view_of(x)).expect("trie")
        })
        .join()
        .expect("the replay does not panic");
        assert!(got == want);
    }
}
