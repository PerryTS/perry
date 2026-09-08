//! `PERRY_GC_DIAG=1`: per copying minor, WHY each surviving byte survived.
//!
//! `[gc-copy-minor]` reports how much survived; it cannot say what kept it
//! alive. On the compiled claude-code TUI the streaming turn scavenges at
//! 57–99 % survival while a heap census puts the true live set at ~45 MB, so
//! at scavenge time something references almost the whole Eden that stops
//! referencing it soon after. The candidates differ in their fix — nepotism
//! through the remembered set (a dead-but-unswept old object whose dirty page
//! still points at young objects), a stack-map root, a registered side-table
//! scanner, a legitimately live render tree — and only attribution tells
//! them apart.
//!
//! Every object the copying minor moves or promotes is charged to the ORIGIN
//! that first reached it:
//!
//! * a direct root: the walk phase the collector is in
//!   (`pin::copying_walk_phase()` — `mutable_root_slots/shadow_stack`,
//!   `mutable_root_slots/native_stack`, `mutable_root_slots/global_root`,
//!   or the registered scanner's name), and for the remembered set the OLD
//!   PARENT'S type (`remembered_set/array`, `remembered_set/object`, …);
//! * a transitive reach: the origin of the worklist entry whose field scan
//!   found it. The worklist carries a parallel origin vector so the drain
//!   propagates it — the collector's own worklist is untouched.
//!
//! Output, after each minor's `[gc-copy-minor]` line: the top rows by bytes
//! as `[gc-survival] minor=N origin=<origin> type=<type> objects= bytes=
//! promoted_bytes=`, then per-origin and per-type totals. Allocation is Rust
//! heap only (no JS-heap allocation inside the collector), and the whole
//! structure exists only while the diag is on — `CopyingNurseryCollector`
//! carries it as `Option<Box<_>>` and every hook is one null check.

use super::*;
use std::collections::HashMap;

#[derive(Default, Clone, Copy)]
struct Row {
    objects: u64,
    bytes: u64,
    promoted_bytes: u64,
}

pub(super) struct SurvivalDiag {
    /// Interned origin names; the index is the origin id.
    names: Vec<String>,
    /// `&'static str` identity → origin id, so a phase name is interned once.
    by_ptr: HashMap<usize, u16>,
    /// Origin ids for `remembered_set/<parent type>`, by parent `obj_type`.
    remembered_ids: Vec<u16>,
    /// The old parent's type while the remembered-set scan visits its slots.
    pub(super) remembered_parent_type: u8,
    /// Origin of the worklist entry currently being drained, if draining.
    drain_origin: Option<u16>,
    /// Parallel to `CopyingNurseryCollector::worklist`.
    worklist_origin: Vec<u16>,
    /// `(origin id << 8) | obj_type` → row.
    rows: HashMap<u32, Row>,
}

impl SurvivalDiag {
    pub(super) fn new() -> Self {
        let mut d = Self {
            names: Vec::new(),
            by_ptr: HashMap::new(),
            remembered_ids: Vec::new(),
            remembered_parent_type: 0,
            drain_origin: None,
            worklist_origin: Vec::new(),
            rows: HashMap::new(),
        };
        for t in 0..=GC_TYPE_MAX as usize {
            let name = gc_type_info(t as u8).map_or("?", |i| i.name);
            let id = d.intern_owned(format!("remembered_set/{name}"));
            d.remembered_ids.push(id);
        }
        d
    }

    fn intern_owned(&mut self, name: String) -> u16 {
        if let Some(i) = self.names.iter().position(|n| *n == name) {
            return i as u16;
        }
        self.names.push(name);
        (self.names.len() - 1) as u16
    }

    fn intern_static(&mut self, name: &'static str) -> u16 {
        let key = name.as_ptr() as usize;
        if let Some(&id) = self.by_ptr.get(&key) {
            return id;
        }
        let id = self.intern_owned(name.to_string());
        self.by_ptr.insert(key, id);
        id
    }

    /// The origin a newly reached object is charged to right now.
    fn current_origin(&mut self) -> u16 {
        if let Some(o) = self.drain_origin {
            return o;
        }
        let phase = super::pin::copying_walk_phase().unwrap_or("unknown");
        if phase == "remembered_set" {
            let t = (self.remembered_parent_type as usize).min(self.remembered_ids.len() - 1);
            return self.remembered_ids[t];
        }
        self.intern_static(phase)
    }

    /// Mirror of `collector.worklist.push(..)`.
    #[inline]
    pub(super) fn note_worklist_push(&mut self) {
        let o = self.current_origin();
        self.worklist_origin.push(o);
    }

    /// The drain is about to scan worklist entry `i`.
    #[inline]
    pub(super) fn begin_drain_entry(&mut self, i: usize) {
        self.drain_origin = self.worklist_origin.get(i).copied();
    }

    pub(super) fn end_drain(&mut self) {
        self.drain_origin = None;
    }

    /// One object of `obj_type` and `bytes` was copied (or promoted).
    #[inline]
    pub(super) fn record(&mut self, obj_type: u8, bytes: usize, promoted: bool) {
        let origin = self.current_origin();
        let key = (u32::from(origin) << 8) | u32::from(obj_type);
        let row = self.rows.entry(key).or_default();
        row.objects += 1;
        row.bytes += bytes as u64;
        if promoted {
            row.promoted_bytes += bytes as u64;
        }
    }

    pub(super) fn report(&self, seq: u64) {
        #[cfg(test)]
        LAST_REPORT.with(|r| {
            *r.borrow_mut() = self
                .rows
                .iter()
                .map(|(k, row)| {
                    (
                        self.names[(k >> 8) as usize].clone(),
                        (k & 0xff) as u8,
                        row.objects,
                        row.bytes,
                        row.promoted_bytes,
                    )
                })
                .collect();
        });
        if self.rows.is_empty() {
            return;
        }
        let mut rows: Vec<(u32, Row)> = self.rows.iter().map(|(k, r)| (*k, *r)).collect();
        rows.sort_by_key(|(_, r)| std::cmp::Reverse(r.bytes));
        let total_bytes: u64 = rows.iter().map(|(_, r)| r.bytes).sum();
        let total_objects: u64 = rows.iter().map(|(_, r)| r.objects).sum();
        eprintln!(
            "[gc-survival] minor={seq} rows={} objects={total_objects} bytes={total_bytes}",
            rows.len()
        );
        for (key, r) in rows.iter().take(24) {
            let origin = &self.names[(key >> 8) as usize];
            let t = (key & 0xff) as u8;
            let tname = gc_type_info(t).map_or("?", |i| i.name);
            eprintln!(
                "[gc-survival]   minor={seq} origin={origin} type={tname} objects={} bytes={} promoted_bytes={}",
                r.objects, r.bytes, r.promoted_bytes
            );
        }
        let mut by_origin: HashMap<u16, Row> = HashMap::new();
        let mut by_type: HashMap<u8, Row> = HashMap::new();
        for (key, r) in &rows {
            let o = by_origin.entry((key >> 8) as u16).or_default();
            o.objects += r.objects;
            o.bytes += r.bytes;
            o.promoted_bytes += r.promoted_bytes;
            let t = by_type.entry((key & 0xff) as u8).or_default();
            t.objects += r.objects;
            t.bytes += r.bytes;
            t.promoted_bytes += r.promoted_bytes;
        }
        let mut by_origin: Vec<_> = by_origin.into_iter().collect();
        by_origin.sort_by_key(|(_, r)| std::cmp::Reverse(r.bytes));
        for (o, r) in by_origin.iter().take(12) {
            eprintln!(
                "[gc-survival]   minor={seq} origin-total={} objects={} bytes={} permille={}",
                self.names[*o as usize],
                r.objects,
                r.bytes,
                if total_bytes > 0 {
                    r.bytes * 1000 / total_bytes
                } else {
                    0
                }
            );
        }
        let mut by_type: Vec<_> = by_type.into_iter().collect();
        by_type.sort_by_key(|(_, r)| std::cmp::Reverse(r.bytes));
        for (t, r) in by_type.iter().take(8) {
            eprintln!(
                "[gc-survival]   minor={seq} type-total={} objects={} bytes={}",
                gc_type_info(*t).map_or("?", |i| i.name),
                r.objects,
                r.bytes
            );
        }
    }
}

crate::perry_thread_local! {
    static MINOR_SEQ: Cell<u64> = const { Cell::new(0) };
}

#[cfg(test)]
crate::perry_thread_local! {
    /// Test-only snapshot of the last report's rows:
    /// `(origin, obj_type, objects, bytes, promoted_bytes)`.
    static LAST_REPORT: RefCell<Vec<(String, u8, u64, u64, u64)>> = const { RefCell::new(Vec::new()) };
}

/// Test-only: the rows of the most recent `report` on this thread.
#[cfg(test)]
pub(super) fn test_last_report() -> Vec<(String, u8, u64, u64, u64)> {
    LAST_REPORT.with(|r| r.borrow().clone())
}

/// Sequence number for the next copying minor's report.
pub(super) fn next_minor_seq() -> u64 {
    MINOR_SEQ.with(|c| {
        let v = c.get() + 1;
        c.set(v);
        v
    })
}
