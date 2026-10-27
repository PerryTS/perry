use super::{Entry, Instant};
use crate::slots::Slots;
/// Four-ary heap with an index for immediate O(log n) cancellation. Keys encode
/// the slot index in their low 32 bits; one deadline per slot.
///
/// `capacity` is a ceiling, not a preallocation: the position index is built in
/// pages as slots are used, and the heap itself grows with the timers actually
/// armed, so a loop that arms none costs nothing for a large ceiling.
pub struct Heap {
    heap: Vec<Entry>,
    /// Each armed slot's position in `heap`; vacant means that slot has no timer.
    positions: Slots<usize>,
}
impl Heap {
    pub fn new(capacity: usize) -> Self {
        Self {
            heap: Vec::with_capacity(crate::slots::PAGE.min(capacity)),
            positions: Slots::new(capacity),
        }
    }
    pub fn len(&self) -> usize {
        self.heap.len()
    }
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
    pub fn next_deadline(&self) -> Option<Instant> {
        self.heap.first().map(|e| e.at)
    }
    pub fn insert(&mut self, id: u64, at: Instant) {
        let i = id as u32 as usize;
        assert!(self.positions[i].is_none(), "one timer per slot");
        self.positions[i] = Some(self.heap.len());
        self.heap.push(Entry { id, at });
        self.up(self.heap.len() - 1);
    }
    pub fn cancel(&mut self, id: u64) -> bool {
        let i = id as u32 as usize;
        let Some(&Some(p)) = self.positions.get(i) else {
            return false;
        };
        if self.heap[p].id != id {
            return false;
        }
        self.heap.swap_remove(p);
        self.positions[i] = None;
        if p < self.heap.len() {
            self.positions[self.heap[p].id as u32 as usize] = Some(p);
            if p > 0 && self.heap[p] < self.heap[(p - 1) / 4] {
                self.up(p);
            } else {
                self.down(p);
            }
        }
        true
    }
    pub fn pop_expired(&mut self, now: Instant) -> Option<(u64, Instant)> {
        let e = *self.heap.first()?;
        if e.at > now {
            return None;
        }
        self.cancel(e.id);
        Some((e.id, e.at))
    }
    fn swap(&mut self, a: usize, b: usize) {
        self.heap.swap(a, b);
        self.positions[self.heap[a].id as u32 as usize] = Some(a);
        self.positions[self.heap[b].id as u32 as usize] = Some(b);
    }
    fn up(&mut self, mut p: usize) {
        while p > 0 {
            let parent = (p - 1) / 4;
            if self.heap[parent] <= self.heap[p] {
                break;
            }
            self.swap(p, parent);
            p = parent;
        }
    }
    fn down(&mut self, mut p: usize) {
        loop {
            let first = p * 4 + 1;
            if first >= self.heap.len() {
                break;
            }
            let mut best = first;
            for c in first + 1..(first + 4).min(self.heap.len()) {
                if self.heap[c] < self.heap[best] {
                    best = c;
                }
            }
            if self.heap[p] <= self.heap[best] {
                break;
            }
            self.swap(p, best);
            p = best;
        }
    }
}
