//! turnloop P3: the owning agent's timer heap and callback queues.
//!
//! Before P3 the runtime kept three process-global `Mutex<Vec<_>>` timer queues
//! (`TIMER_QUEUE`, `CALLBACK_TIMERS`, `INTERVAL_TIMERS`). Every tick, every
//! next-deadline computation and every liveness question walked them end to
//! end, filtering on owner, `cleared` and ref state; a clear was a `retain`
//! over the whole queue, and firing a batch meant detaching a `Vec` of records
//! the collector could not see (#8036). Ordering across the three was by queue,
//! not by deadline, so an expired `setInterval(g, 3)` fired after an expired
//! `setTimeout(f, 5)` — Node walks one expiry-ordered structure and runs `g`
//! first.
//!
//! This module replaces all three with **one store per JS agent** (DESIGN
//! §5a.7, "Per-agent timers"):
//!
//! - a **slab** of entries, so an entry's identity is a stable index that
//!   survives every reordering (the incremental GC scan resumes on it);
//! - two **binary min-heaps** of slab indices ordered by `(deadline, seq)` —
//!   one for ref'd entries, one for unref'd ones — giving O(log n) insert,
//!   O(log n) cancel with the entry removed immediately rather than
//!   tombstoned (DESIGN D6), and an O(1) earliest deadline;
//! - a **check queue** (`setImmediate` and the native completion callbacks
//!   that share its delivery), FIFO, drained by the check phase;
//! - an id index, so `clearTimeout`/`ref`/`unref`/`refresh` are O(log n)
//!   lookups instead of reverse scans of a `Vec`.
//!
//! Splitting the store by agent is what removes the owner filter from every
//! read: `owns(owner)` is exactly `owner == current_agent()` (`agent.rs`), so
//! selecting the current agent's partition answers the same question
//! structurally. An agent that exits drops its whole partition.
//!
//! **Keep-alive counters are republished, not maintained pairwise.** P0's
//! counters were incremented and decremented at every mutation site, with a
//! debug assertion re-deriving them because an unpaired site is invisible in
//! release. Here the counters are derived from the partition itself after every
//! mutation (`publish_primary`), so there is no pairing to get wrong.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::agent::{AgentId, PRIMARY_AGENT};
use crate::promise::Promise;

/// Which JS API produced an entry. The class decides which phase runs it and
/// which slots the collector visits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Class {
    /// `js_set_timeout*` — resolves a promise, runs no user callback.
    Promise,
    /// `setTimeout(fn, delay)`.
    Timeout,
    /// `setInterval(fn, period)`.
    Interval,
    /// `setImmediate(fn)`. Runs in the check phase, never in the timers phase.
    Immediate,
    /// A native completion callback (`fs`, `dns`, `crypto`) — Node delivers
    /// these in the **poll** phase, ahead of the same iteration's immediates.
    Pending,
}

impl Class {
    /// Entries the timers phase owns.
    pub(super) fn is_timer(self) -> bool {
        matches!(self, Class::Promise | Class::Timeout | Class::Interval)
    }
}

/// Not in any heap.
const NO_POS: usize = usize::MAX;

/// One scheduled timer or immediate.
///
/// Flat rather than an enum per class: the collector visits the same slots on
/// every entry (guarded by `class`), which is what keeps the incremental scan a
/// simple index walk.
pub(super) struct Entry {
    /// #10447: pins this id's ref state in the handle registry for exactly as
    /// long as the entry is queued. Dropping the entry — `take`, a slab slot
    /// being reused, `purge_agent_timers` — retires the id, so no removal site
    /// can forget to, and a still-queued timer can never be evicted out from
    /// under `.hasRef()`/`.ref()`/`.unref()`.
    ///
    /// `None` for promise timers, which have no JS handle to query.
    /// Declared FIRST so it drops last, after the fields it guards.
    pub(super) _scheduled: Option<super::ref_states::ScheduledTimerId>,
    /// JS handle id. Promise timers have no JS handle and use 0.
    pub(super) id: i64,
    pub(super) class: Class,
    /// When the timers phase may run this entry. Meaningless for `Immediate`.
    pub(super) deadline: Instant,
    /// Creation order, the tie-break for equal deadlines (Node fires
    /// same-deadline timers in creation order).
    pub(super) seq: u64,
    /// The delay `Timeout.refresh()` re-applies, and an interval's period.
    pub(super) delay_ms: u64,
    pub(super) refed: bool,
    /// `Class::Promise` only.
    pub(super) promise: *mut Promise,
    /// `Class::Promise` only: the NaN-boxed resolution value.
    pub(super) value: f64,
    /// Closure pointer for the callback classes; 0 for `Class::Promise`.
    pub(super) callback: i64,
    pub(super) args: Vec<f64>,
    pub(super) context: crate::async_context::AsyncContextSnapshot,
    pub(super) async_id: u64,
    pub(super) trigger_async_id: u64,
    /// Position in the heap that currently holds this entry, or [`NO_POS`].
    heap_pos: usize,
}

// SAFETY: `promise`, `callback` and any NaN-boxed `args` point into the arena
// of the agent whose partition holds this entry. The partition is selected by
// `current_agent()` at every read, so an entry is only ever dereferenced by a
// thread acting for its owner — the property #6185's owner tag asserted and
// this structure enforces.
unsafe impl Send for Entry {}

impl Entry {
    /// A callback entry with everything but the scheduling fields filled in.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn callback(
        id: i64,
        class: Class,
        deadline: Instant,
        delay_ms: u64,
        callback: i64,
        args: Vec<f64>,
        context: crate::async_context::AsyncContextSnapshot,
        async_id: u64,
        trigger_async_id: u64,
        scheduled: Option<super::ref_states::ScheduledTimerId>,
    ) -> Self {
        Self {
            _scheduled: scheduled,
            id,
            class,
            deadline,
            seq: 0,
            delay_ms,
            refed: true,
            promise: std::ptr::null_mut(),
            value: 0.0,
            callback,
            args,
            context,
            async_id,
            trigger_async_id,
            heap_pos: NO_POS,
        }
    }

    /// A copy of an interval entry to put back in the heap while the popped
    /// one is dispatched. libuv re-arms a repeating timer before calling its
    /// callback, so `clearInterval` from inside the callback has something to
    /// cancel; the dispatching copy is rooted separately for the call.
    ///
    /// Takes `&mut self` to MOVE the #10447 pin into the copy. The copy is what
    /// goes back in the heap and is therefore the live timer; `self` is
    /// dispatched and then dropped. Leaving the pin on `self` would retire the
    /// id the moment the callback finished, while the re-armed entry was still
    /// queued — and `ScheduledTimerId` is deliberately `!Clone`, so the
    /// borrow checker does not let the mistake be written accidentally.
    pub(super) fn duplicate_for_rearm(&mut self) -> Self {
        Self {
            _scheduled: self._scheduled.take(),
            id: self.id,
            class: self.class,
            deadline: self.deadline,
            seq: 0,
            delay_ms: self.delay_ms,
            refed: self.refed,
            promise: self.promise,
            value: self.value,
            callback: self.callback,
            args: self.args.clone(),
            context: self.context.clone(),
            async_id: self.async_id,
            trigger_async_id: self.trigger_async_id,
            heap_pos: NO_POS,
        }
    }

    pub(super) fn promise(
        deadline: Instant,
        promise: *mut Promise,
        value: f64,
        refed: bool,
    ) -> Self {
        Self {
            // A promise timer has no JS handle, so nothing can query its ref
            // state and there is nothing to pin.
            _scheduled: None,
            id: 0,
            class: Class::Promise,
            deadline,
            seq: 0,
            delay_ms: 0,
            refed,
            promise,
            value,
            callback: 0,
            args: Vec::new(),
            context: crate::async_context::AsyncContextSnapshot::default(),
            async_id: 0,
            trigger_async_id: 0,
            heap_pos: NO_POS,
        }
    }
}

/// One agent's timers. Everything here belongs to one JS heap.
#[derive(Default)]
pub(super) struct AgentTimers {
    slab: Vec<Option<Entry>>,
    free: Vec<usize>,
    /// Slab indices, min-heap on `(deadline, seq)`, ref'd entries only.
    refed: Vec<usize>,
    /// Slab indices, min-heap on `(deadline, seq)`, unref'd entries only.
    unrefed: Vec<usize>,
    /// Check-phase queue in scheduling order. Holds slab indices; a cancelled
    /// entry leaves a `None` slab slot behind, skipped on pop.
    check: VecDeque<usize>,
    /// Ref'd entries currently in `check`.
    refed_check: usize,
    /// Live (not cancelled) entries in `check`, and in the two poll queues.
    /// Counted rather than derived: `check_pending` is asked on every schedule
    /// and every cancel — it is what keeps the loop's park decision and its
    /// armed deadline in step — and a scan there would make a program that
    /// queues n callbacks cost O(n^2).
    check_live: usize,
    poll_live: usize,
    /// Native completion callbacks waiting for the poll phase that will run
    /// them, and the ones still waiting to become eligible. See
    /// [`AgentTimers::promote_pending`].
    poll_ready: VecDeque<usize>,
    poll_staged: VecDeque<usize>,
    by_id: BTreeMap<i64, usize>,
    next_seq: u64,
}

impl AgentTimers {
    fn alloc(&mut self, mut entry: Entry) -> usize {
        entry.seq = self.next_seq;
        self.next_seq += 1;
        entry.heap_pos = NO_POS;
        let id = entry.id;
        let index = match self.free.pop() {
            Some(index) => {
                self.slab[index] = Some(entry);
                index
            }
            None => {
                self.slab.push(Some(entry));
                self.slab.len() - 1
            }
        };
        if id != 0 {
            self.by_id.insert(id, index);
        }
        index
    }

    fn take(&mut self, index: usize) -> Option<Entry> {
        let entry = self.slab.get_mut(index)?.take()?;
        self.free.push(index);
        if entry.id != 0 {
            // Only unmap the id if it still points here: `refresh` can rebuild
            // an entry under the same id, and a later removal of the old index
            // must not orphan the new one.
            if self.by_id.get(&entry.id) == Some(&index) {
                self.by_id.remove(&entry.id);
            }
        }
        Some(entry)
    }

    /// Order key: earliest deadline first, then creation order.
    fn key(&self, index: usize) -> (Instant, u64) {
        let entry = self.slab[index].as_ref().expect("heap index is live");
        (entry.deadline, entry.seq)
    }

    fn heap(&mut self, refed: bool) -> &mut Vec<usize> {
        if refed {
            &mut self.refed
        } else {
            &mut self.unrefed
        }
    }

    fn set_pos(&mut self, index: usize, pos: usize) {
        if let Some(entry) = self.slab[index].as_mut() {
            entry.heap_pos = pos;
        }
    }

    fn heap_push(&mut self, index: usize) {
        let refed = self.slab[index].as_ref().expect("live entry").refed;
        let pos = if refed {
            self.refed.len()
        } else {
            self.unrefed.len()
        };
        self.heap(refed).push(index);
        self.set_pos(index, pos);
        self.sift_up(refed, pos);
    }

    fn sift_up(&mut self, refed: bool, mut pos: usize) {
        while pos > 0 {
            let parent = (pos - 1) / 2;
            let (a, b) = {
                let heap = if refed { &self.refed } else { &self.unrefed };
                (heap[pos], heap[parent])
            };
            if self.key(a) >= self.key(b) {
                break;
            }
            {
                let heap = self.heap(refed);
                heap.swap(pos, parent);
            }
            self.set_pos(a, parent);
            self.set_pos(b, pos);
            pos = parent;
        }
    }

    fn sift_down(&mut self, refed: bool, mut pos: usize) {
        loop {
            let len = if refed {
                self.refed.len()
            } else {
                self.unrefed.len()
            };
            let mut best = pos;
            for child in [pos * 2 + 1, pos * 2 + 2] {
                if child < len {
                    let (c, b) = {
                        let heap = if refed { &self.refed } else { &self.unrefed };
                        (heap[child], heap[best])
                    };
                    if self.key(c) < self.key(b) {
                        best = child;
                    }
                }
            }
            if best == pos {
                return;
            }
            let (a, b) = {
                let heap = if refed { &self.refed } else { &self.unrefed };
                (heap[pos], heap[best])
            };
            {
                let heap = self.heap(refed);
                heap.swap(pos, best);
            }
            self.set_pos(a, best);
            self.set_pos(b, pos);
            pos = best;
        }
    }

    /// Detach `index` from whichever heap holds it. Cancelled entries leave no
    /// tombstone behind (DESIGN D6).
    fn heap_detach(&mut self, index: usize) {
        let (refed, pos) = {
            let Some(entry) = self.slab.get(index).and_then(|e| e.as_ref()) else {
                return;
            };
            (entry.refed, entry.heap_pos)
        };
        if pos == NO_POS {
            return;
        }
        let last = {
            let heap = self.heap(refed);
            let last = heap.pop().expect("heap holds the detached entry");
            if last == index {
                self.set_pos(index, NO_POS);
                return;
            }
            heap[pos] = last;
            last
        };
        self.set_pos(index, NO_POS);
        self.set_pos(last, pos);
        self.sift_up(refed, pos);
        self.sift_down(refed, pos);
    }

    fn heap_top(&self, refed: bool) -> Option<usize> {
        let heap = if refed { &self.refed } else { &self.unrefed };
        heap.first().copied()
    }

    /// Insert a timers-phase entry.
    pub(super) fn insert_timer(&mut self, entry: Entry) -> usize {
        debug_assert!(entry.class.is_timer());
        let index = self.alloc(entry);
        self.heap_push(index);
        index
    }

    /// Append a check-phase entry.
    pub(super) fn insert_check(&mut self, entry: Entry) -> usize {
        debug_assert_eq!(entry.class, Class::Immediate);
        let refed = entry.refed;
        let index = self.alloc(entry);
        self.check.push_back(index);
        self.refed_check += usize::from(refed);
        self.check_live += 1;
        index
    }

    /// The two heap roots, so the caller can decide whether the unref'd one is
    /// even a candidate before paying for that question.
    ///
    /// `should_run_unref_*` reaches into stdlib (`js_stdlib_has_active_handles`,
    /// which walks the WS/HTTP/pump registries), and the deadline is recomputed
    /// on every schedule and every cancel to keep the loop's armed timer in
    /// step. A program with no unref'd timer — the overwhelmingly common case —
    /// must not pay for it.
    pub(super) fn deadline_candidates(&self) -> (Option<Instant>, Option<Instant>) {
        (
            self.heap_top(true).map(|i| self.key(i).0),
            self.heap_top(false).map(|i| self.key(i).0),
        )
    }

    /// The earliest timers-phase deadline this agent must wake for.
    ///
    /// `allow_unref` mirrors `should_run_unref_*`: an unref'd timer still fires
    /// while some other source keeps the loop alive, but never keeps it alive
    /// by itself, so when nothing else does it must not contribute a deadline.
    #[cfg(test)]
    pub(super) fn next_deadline(&self, allow_unref: bool) -> Option<Instant> {
        let refed = self.heap_top(true).map(|i| self.key(i).0);
        let unrefed = if allow_unref {
            self.heap_top(false).map(|i| self.key(i).0)
        } else {
            None
        };
        match (refed, unrefed) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Remove and return the earliest entry due at `now`, or `None`.
    ///
    /// `horizon` is the phase's snapshot boundary: an entry created during the
    /// phase has `seq >= horizon` and waits for the next iteration, as in Node.
    /// A newly created entry's deadline is never earlier than the phase clock
    /// read, so it can only ever be the *last* of the due entries — stopping at
    /// it therefore skips nothing that was already due.
    /// Ref state does not gate *firing*: an unref'd timer whose deadline has
    /// passed runs like any other, and only fails to run when nothing keeps the
    /// loop alive to reach this phase at all — which is exactly Node's rule
    /// (measured: an unref'd timer fires if a ref'd one holds the loop open
    /// past its deadline, and an unref'd interval alone never ticks). Ref state
    /// gates the *deadline* instead, in `next_deadline`.
    pub(super) fn pop_due(&mut self, now: Instant, horizon: u64) -> Option<Entry> {
        let refed = self.heap_top(true).filter(|&i| self.key(i).0 <= now);
        let unrefed = self.heap_top(false).filter(|&i| self.key(i).0 <= now);
        let index = match (refed, unrefed) {
            (Some(a), Some(b)) => {
                if self.key(a) <= self.key(b) {
                    a
                } else {
                    b
                }
            }
            (a, b) => a.or(b)?,
        };
        if self.key(index).1 >= horizon {
            return None;
        }
        self.heap_detach(index);
        self.take(index)
    }

    /// Remove and return the next check-phase entry whose `seq` is below
    /// `horizon`. The horizon is the check phase's snapshot: an immediate
    /// scheduled *by* a check callback runs on the next turn, as in Node.
    pub(super) fn pop_check(&mut self, horizon: u64) -> Option<Entry> {
        loop {
            let index = *self.check.front()?;
            match self.slab.get(index).and_then(|e| e.as_ref()) {
                // Cancelled: its slab slot is already gone. Drop the placeholder.
                None => {
                    self.check.pop_front();
                }
                Some(entry) if entry.seq >= horizon => return None,
                Some(entry) => {
                    let refed = entry.refed;
                    self.check.pop_front();
                    self.refed_check -= usize::from(refed);
                    self.check_live -= 1;
                    return self.take(index);
                }
            }
        }
    }

    /// Queue a native completion callback for a later poll phase.
    ///
    /// Perry performs the `fs`/`dns`/`crypto` syscall **eagerly** and defers
    /// only the callback, so the completion is already available at the moment
    /// the JS call returns. Node's is not: the operation is still on the
    /// threadpool, so its callback cannot be delivered in the poll phase that
    /// is already running, and a `setImmediate` queued beside it wins — 10/10
    /// runs, in either registration order (measured on 26.5.1).
    ///
    /// Staging reproduces that one turn of latency exactly: an entry queued
    /// here is passed over by the poll phase in flight (or, for a top-level
    /// call, by the first one) and runs in the next. It is not a delay Perry
    /// invents — it is the delay Perry's eager syscall removed.
    pub(super) fn insert_pending(&mut self, entry: Entry) -> usize {
        debug_assert_eq!(entry.class, Class::Pending);
        let index = self.alloc(entry);
        self.poll_staged.push_back(index);
        self.poll_live += 1;
        index
    }

    /// Take the next native completion callback the poll phase may run.
    pub(super) fn pop_poll(&mut self) -> Option<Entry> {
        loop {
            let index = *self.poll_ready.front()?;
            match self.slab.get(index).and_then(|e| e.as_ref()) {
                // Cancelled: drop the placeholder and look at the next one.
                None => {
                    self.poll_ready.pop_front();
                }
                Some(_) => {
                    self.poll_ready.pop_front();
                    self.poll_live -= 1;
                    return self.take(index);
                }
            }
        }
    }

    /// Make the staged native completions eligible for the NEXT poll phase.
    /// Called at the end of each poll phase, after `pop_poll` has drained what
    /// this one may run.
    pub(super) fn promote_pending(&mut self) {
        self.poll_ready.append(&mut self.poll_staged);
    }

    /// Native completion callbacks queued but not yet run, in either queue.
    pub(super) fn poll_pending(&self) -> bool {
        debug_assert_eq!(
            self.poll_live,
            self.poll_ready
                .iter()
                .chain(self.poll_staged.iter())
                .filter(|&&i| self.slab.get(i).is_some_and(Option::is_some))
                .count(),
            "poll_live drifted from the poll queues"
        );
        self.poll_live != 0
    }

    /// The sequence number the next scheduled entry will get — the check
    /// phase's snapshot boundary.
    pub(super) fn seq_horizon(&self) -> u64 {
        self.next_seq
    }

    /// Remove the entry with `id`, if its class is admitted by `accept`.
    pub(super) fn remove_by_id(
        &mut self,
        id: i64,
        accept: impl Fn(Class) -> bool,
    ) -> Option<Entry> {
        let index = *self.by_id.get(&id)?;
        let class = self.slab.get(index)?.as_ref()?.class;
        if !accept(class) {
            return None;
        }
        if class.is_timer() {
            self.heap_detach(index);
        } else if class == Class::Pending {
            // Leave the queue placeholder; `pop_poll` skips an emptied slot.
            self.poll_live -= 1;
        } else {
            // Leave the queue placeholder: `pop_check` skips an emptied slot.
            // Removing it here would be O(n) in the queue length for no gain.
            let refed = self.slab[index].as_ref().expect("live entry").refed;
            self.refed_check -= usize::from(refed);
            self.check_live -= 1;
        }
        self.take(index)
    }

    /// Apply `ref()`/`unref()` to a queued entry. Returns whether one was found.
    pub(super) fn set_ref(&mut self, id: i64, refed: bool) -> bool {
        let Some(&index) = self.by_id.get(&id) else {
            return false;
        };
        let Some(entry) = self.slab.get_mut(index).and_then(|e| e.as_mut()) else {
            return false;
        };
        if entry.refed == refed {
            return true;
        }
        let class = entry.class;
        if class.is_timer() {
            self.heap_detach(index);
            self.slab[index].as_mut().expect("live entry").refed = refed;
            self.heap_push(index);
        } else {
            entry.refed = refed;
            if refed {
                self.refed_check += 1;
            } else {
                self.refed_check -= 1;
            }
        }
        true
    }

    /// `Timeout.refresh()`: re-arm the entry at `now + delay`. Returns whether
    /// a queued entry was found.
    ///
    /// Ref state is **preserved**, not reset: Node's `refresh()` re-inserts the
    /// timer into its list and never touches `[kRefed]`, so an unref'd handle
    /// stays unref'd across a refresh and still reports `hasRef() === false`
    /// (measured on 26.5.1). Perry used to force it back to ref'd.
    pub(super) fn refresh(&mut self, id: i64, now: Instant) -> bool {
        let Some(&index) = self.by_id.get(&id) else {
            return false;
        };
        let Some(entry) = self.slab.get(index).and_then(|e| e.as_ref()) else {
            return false;
        };
        if !entry.class.is_timer() {
            return false;
        }
        let delay = std::time::Duration::from_millis(entry.delay_ms);
        self.heap_detach(index);
        {
            let entry = self.slab[index].as_mut().expect("live entry");
            entry.deadline = now + delay;
            entry.seq = self.next_seq;
        }
        self.next_seq += 1;
        self.heap_push(index);
        true
    }

    /// Re-arm a fired interval one period past the timers phase's clock read.
    /// libuv restarts a repeating timer from the loop's cached time, which is
    /// strictly in the past-or-now, so the re-armed deadline is always after
    /// the phase's horizon and an interval can never fire twice in one phase.
    pub(super) fn rearm_interval(&mut self, mut entry: Entry, phase_now: Instant) {
        entry.deadline = phase_now + std::time::Duration::from_millis(entry.delay_ms.max(1));
        entry.heap_pos = NO_POS;
        self.insert_timer(entry);
    }

    pub(super) fn refed_timers(&self) -> usize {
        self.refed.len()
    }

    #[cfg(test)]
    pub(super) fn refed_check(&self) -> usize {
        self.refed_check
    }

    /// Whether the loop has work that must run on the very next turn, with no
    /// park in between: a queued immediate (Node computes a zero poll timeout
    /// when the immediate queue is non-empty) or a native completion callback
    /// waiting for its poll phase.
    pub(super) fn check_pending(&self) -> bool {
        debug_assert_eq!(
            self.check_live,
            self.check
                .iter()
                .filter(|&&i| self.slab.get(i).is_some_and(Option::is_some))
                .count(),
            "check_live drifted from the check queue"
        );
        self.check_live != 0 || self.poll_pending()
    }

    /// Any entry at all, including unref'd ones: the "is a timer phase worth
    /// running" predicate.
    pub(super) fn any_pending(&self) -> bool {
        !self.refed.is_empty() || !self.unrefed.is_empty() || self.check_pending()
    }

    /// Ref'd check-phase and poll-phase entries: the keep-alive contribution of
    /// everything that is not a timer. A native completion callback always
    /// keeps the loop alive — dropping one would lose the completion.
    pub(super) fn refed_non_timer(&self) -> usize {
        self.refed_check + usize::from(self.poll_pending())
    }

    /// Queued `setTimeout` and `setInterval` entries — `process.getActiveResourcesInfo`.
    pub(super) fn timeout_resource_count(&self) -> usize {
        self.slab
            .iter()
            .flatten()
            .filter(|e| matches!(e.class, Class::Timeout | Class::Interval))
            .count()
    }

    /// Bytes this partition's own allocations hold, for `PERRY_GC_CENSUS`.
    fn census(&self) -> (usize, usize) {
        let live = self.slab.iter().flatten().count();
        let args: usize = self
            .slab
            .iter()
            .flatten()
            .map(|e| e.args.capacity() * std::mem::size_of::<f64>())
            .sum();
        let bytes = self.slab.capacity() * std::mem::size_of::<Option<Entry>>()
            + (self.refed.capacity() + self.unrefed.capacity() + self.free.capacity())
                * std::mem::size_of::<usize>()
            + (self.check.capacity() + self.poll_ready.capacity() + self.poll_staged.capacity())
                * std::mem::size_of::<usize>()
            + args;
        (live, bytes)
    }

    /// Drop every entry (test scaffolding for the GC root-scanner fixtures).
    #[cfg(test)]
    pub(super) fn test_clear(&mut self) {
        self.slab.clear();
        self.free.clear();
        self.refed.clear();
        self.unrefed.clear();
        self.check.clear();
        self.poll_ready.clear();
        self.poll_staged.clear();
        self.refed_check = 0;
        self.check_live = 0;
        self.poll_live = 0;
        self.by_id.clear();
    }

    #[cfg(test)]
    pub(super) fn test_find_by_id(&self, id: i64) -> Option<&Entry> {
        let index = *self.by_id.get(&id)?;
        self.slab.get(index)?.as_ref()
    }

    #[cfg(test)]
    pub(super) fn test_last_of_class(&self, class: Class) -> Option<&Entry> {
        self.slab
            .iter()
            .flatten()
            .filter(|e| e.class == class)
            .max_by_key(|e| e.seq)
    }

    /// Rebuild the partition keeping only the entries `keep` admits.
    #[cfg(test)]
    pub(super) fn test_retain(&mut self, keep: impl Fn(&Entry) -> bool) {
        let kept: Vec<Entry> = self
            .slab
            .iter_mut()
            .filter_map(|slot| slot.take())
            .filter(|entry| keep(entry))
            .collect();
        self.test_clear();
        for entry in kept {
            match entry.class {
                c if c.is_timer() => {
                    self.insert_timer(entry);
                }
                Class::Pending => {
                    self.insert_pending(entry);
                }
                _ => {
                    self.insert_check(entry);
                }
            };
        }
    }

    /// Slab length, for the incremental GC scan's index walk.
    pub(super) fn slab_len(&self) -> usize {
        self.slab.len()
    }

    pub(super) fn slab_entry_mut(&mut self, index: usize) -> Option<&mut Entry> {
        self.slab.get_mut(index).and_then(|e| e.as_mut())
    }
}

/// Every agent's timers. One lock: the partitions are disjoint and a thread
/// only ever touches its own agent's, so contention is between a JS thread and
/// a pump acting for the same agent (Android), which is the case that has to
/// serialize anyway.
pub(super) struct Store {
    agents: BTreeMap<AgentId, AgentTimers>,
}

impl Store {
    const fn new() -> Self {
        Self {
            agents: BTreeMap::new(),
        }
    }
}

per_test_global!(pub(super) static STORE: Mutex<Store> = Mutex::new(Store::new()));

// The primary agent's counters, republished from its partition after every
// mutation. The generated event loop asks these several times per turn; a
// mutex plus a map lookup per question is what the P0 counters existed to
// avoid. Worker agents take the lock (there are at most a handful of them and
// they do not run the generated loop's liveness disjunction).
per_test_global!(
    static PRIMARY_REFED_TIMERS: AtomicUsize = AtomicUsize::new(0);
    static PRIMARY_REFED_CHECK: AtomicUsize = AtomicUsize::new(0);
    static PRIMARY_CHECK_PENDING: AtomicUsize = AtomicUsize::new(0);
    static PRIMARY_ANY_PENDING: AtomicUsize = AtomicUsize::new(0);
);

fn publish_primary(timers: &AgentTimers) {
    PRIMARY_REFED_TIMERS.store(timers.refed_timers(), Ordering::Release);
    PRIMARY_REFED_CHECK.store(timers.refed_non_timer(), Ordering::Release);
    PRIMARY_CHECK_PENDING.store(usize::from(timers.check_pending()), Ordering::Release);
    PRIMARY_ANY_PENDING.store(usize::from(timers.any_pending()), Ordering::Release);
}

/// Run `f` against the calling agent's partition, creating it on first use.
///
/// The counters are republished on the way out, so no caller has to remember to
/// pair an increment with a decrement.
pub(super) fn with_current<R>(f: impl FnOnce(&mut AgentTimers) -> R) -> R {
    let agent = crate::agent::current_agent();
    let mut store = STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let timers = store.agents.entry(agent).or_default();
    let result = f(timers);
    if agent == PRIMARY_AGENT {
        publish_primary(timers);
    }
    result
}

/// Run `f` against the calling agent's partition only if it already exists.
/// Reads take this so a question never allocates a partition.
pub(super) fn with_current_existing<R>(f: impl FnOnce(&AgentTimers) -> R) -> Option<R> {
    let agent = crate::agent::current_agent();
    let store = STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.agents.get(&agent).map(f)
}

/// Drop an exited agent's timers. Its arena is about to be unmapped, so nothing
/// in the partition can ever legally run again (`agent::retire_agent`).
pub(crate) fn purge_agent(agent: AgentId) {
    let mut store = STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.agents.remove(&agent);
    if agent == PRIMARY_AGENT {
        publish_primary(&AgentTimers::default());
    }
}

/// O(1) for the primary agent: are there ref'd timers keeping it alive?
pub(super) fn has_refed_timers() -> bool {
    if crate::agent::current_agent() == PRIMARY_AGENT {
        return PRIMARY_REFED_TIMERS.load(Ordering::Acquire) != 0;
    }
    with_current_existing(|t| t.refed_timers() != 0).unwrap_or(false)
}

/// O(1) for the primary agent: are there ref'd check-phase entries?
pub(super) fn has_refed_check() -> bool {
    if crate::agent::current_agent() == PRIMARY_AGENT {
        return PRIMARY_REFED_CHECK.load(Ordering::Acquire) != 0;
    }
    with_current_existing(|t| t.refed_non_timer() != 0).unwrap_or(false)
}

/// Whether the check phase has work queued, ref'd or not. The event loop skips
/// its park while this is true.
pub(super) fn check_pending() -> bool {
    if crate::agent::current_agent() == PRIMARY_AGENT {
        return PRIMARY_CHECK_PENDING.load(Ordering::Acquire) != 0;
    }
    with_current_existing(|t| t.check_pending()).unwrap_or(false)
}

/// Whether anything at all is queued for this agent.
pub(crate) fn any_pending() -> bool {
    if crate::agent::current_agent() == PRIMARY_AGENT {
        return PRIMARY_ANY_PENDING.load(Ordering::Acquire) != 0;
    }
    with_current_existing(|t| t.any_pending()).unwrap_or(false)
}

/// `PERRY_GC_CENSUS`: one row for the calling agent's timer store.
pub(super) fn census_rows() -> Vec<crate::gc::census::SideTableRow> {
    match with_current_existing(|timers| timers.census()) {
        Some((live, bytes)) => vec![("timer.agent_store", live, bytes)],
        None => Vec::new(),
    }
}

#[cfg(test)]
pub(super) fn reset_for_test() {
    let mut store = STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.agents.clear();
    publish_primary(&AgentTimers::default());
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
