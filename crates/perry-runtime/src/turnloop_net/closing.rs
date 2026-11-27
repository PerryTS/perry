//! The close-callbacks phase of net sockets, link and id routes alike.
//!
//! Node emits a socket's `'close'` from its handle's close callback, in the
//! phase after check, and libuv runs that phase newest close first because it
//! prepends each closing handle. A close the phase itself starts waits for the
//! next one. Every close is queued here, in call order: a handle's entry
//! becomes ready when the driver's `Closed` arrives, and a link connect still
//! resolving has no handle, so its entry is ready at once. The end of each
//! check phase turns the driver once for the `Closed` of handles closed since
//! the last poll, then delivers the ready entries queued before it, newest
//! first, each followed by a tick and microtask checkpoint. A link entry holds
//! a resource ref on its cell until it is delivered.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::*;
use crate::native_payload::OwnerLink;

/// Who a queued close is delivered to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Owner {
    /// A link route's payload cell.
    Cell(usize),
    /// An id route's binding id.
    Id(i64),
}

struct Entry {
    owner: Owner,
    /// The link route, or the id route's subsystem.
    route: u8,
    /// The handle whose `Closed` this entry waits for; `None` when the
    /// connect was still resolving.
    handle: Option<Handle>,
    /// The binding released the payload when it closed: any payload on the
    /// cell at delivery belongs to a newer incarnation.
    released: bool,
    /// The `Closed` arrived after the owner had moved to a newer incarnation.
    stale: bool,
}

/// Entries by sequence number (call order), with the ready ones and the
/// waiting handles indexed, so every step is O(log n).
#[derive(Default)]
struct Queue {
    next_seq: u64,
    entries: BTreeMap<u64, Entry>,
    ready: BTreeSet<u64>,
    waiting: HashMap<(Owner, [u32; 4]), u64>,
}

impl Queue {
    fn push(&mut self, entry: Entry, ready: bool) {
        let seq = self.next_seq;
        self.next_seq += 1;
        match (ready, entry.handle) {
            (false, Some(handle)) => {
                self.waiting
                    .insert((entry.owner, transport::handle_parts(handle)), seq);
            }
            _ => {
                self.ready.insert(seq);
            }
        }
        self.entries.insert(seq, entry);
    }
}

crate::perry_thread_local! {
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_tlnet")]
    static QUEUE: RefCell<Queue> = RefCell::new(Queue::default());
}

/// Owe `link`'s `Closed` on `route` to the close-callbacks phase: the
/// driver's `Closed` for `handle` makes it ready, and a resolving connect
/// (`None`) is ready now. `released` says the binding releases the payload
/// with the close. Returns whether a new entry was queued; the caller then
/// holds the ref that pins the cell. A handle already waiting keeps its entry,
/// which only learns that the payload is released.
pub(super) fn queue(link: OwnerLink, route: u8, handle: Option<Handle>, released: bool) -> bool {
    queue_owner(Owner::Cell(link.0), route, handle, released)
}

/// Owe id `id`'s `Closed` for `handle` to the close-callbacks phase, which
/// emits it to `subsystem`.
pub(super) fn queue_id(id: i64, subsystem: u8, handle: Handle) {
    queue_owner(Owner::Id(id), subsystem, Some(handle), false);
}

fn queue_owner(owner: Owner, route: u8, handle: Option<Handle>, released: bool) -> bool {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let waiting =
            handle.and_then(|h| q.waiting.get(&(owner, transport::handle_parts(h))).copied());
        if let Some(seq) = waiting {
            if let Some(entry) = q.entries.get_mut(&seq) {
                entry.released |= released;
            }
            return false;
        }
        let entry = Entry {
            owner,
            route,
            handle,
            released,
            stale: false,
        };
        q.push(entry, handle.is_none());
        true
    })
}

/// Owe the close phase a `Closed` that arrived without a queued close.
pub(super) fn queue_closed(owner: Owner, route: u8, handle: Option<Handle>, stale: bool) {
    let entry = Entry {
        owner,
        route,
        handle,
        released: false,
        stale,
    };
    QUEUE.with(|q| q.borrow_mut().push(entry, true));
}

/// Mark the entry waiting for `handle`'s `Closed` ready. Returns whether one
/// was waiting.
pub(super) fn mark_closed(owner: Owner, handle: Handle, stale: bool) -> bool {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let Some(seq) = q.waiting.remove(&(owner, transport::handle_parts(handle))) else {
            return false;
        };
        if let Some(entry) = q.entries.get_mut(&seq) {
            entry.stale = stale;
        }
        q.ready.insert(seq);
        true
    })
}

/// Whether an entry waits for `handle`'s `Closed` on `owner`.
pub(super) fn awaits(owner: Owner, handle: Handle) -> bool {
    QUEUE.with(|q| {
        q.borrow()
            .waiting
            .contains_key(&(owner, transport::handle_parts(handle)))
    })
}

/// The owner a net completion names.
fn owner_of(completion: &Completion) -> Option<Owner> {
    match transport::link_cell(completion) {
        Some(cell) => Some(Owner::Cell(cell)),
        None => Some(Owner::Id(token_parts(completion.token).1)),
    }
}

/// Whether `completion` is the `Closed` of a handle a close queued.
fn is_queued_close(completion: &Completion) -> bool {
    if transport::owns(completion.token) {
        return transport::is_queued_close(completion);
    }
    let (op, id) = token_parts(completion.token);
    op == OP_CLOSE
        && matches!(completion.result, OpResult::Closed)
        && completion.handle.is_some_and(|h| awaits(Owner::Id(id), h))
}

/// Whether `earlier`, staged ahead of the queued `closed` of its owner, is
/// dispatched with it. An id entry lives until its `Closed`, so every result
/// of the closing handle still reaches its binding first.
fn retires_with(earlier: &Completion, closed: &Completion) -> bool {
    if transport::owns(closed.token) {
        return transport::retires_with(earlier, closed);
    }
    matches!(
        earlier.result,
        OpResult::Closed | OpResult::Cancelled | OpResult::Stopped
    ) || earlier.handle.is_some() && earlier.handle == closed.handle
}

/// Whether a close is owed at all: the loop stays alive for it.
pub(crate) fn pending() -> bool {
    QUEUE.with(|q| q.try_borrow().is_ok_and(|q| !q.entries.is_empty()))
}

/// Whether a close is ready for the next close-callbacks phase: the loop must
/// not park ahead of it.
pub(crate) fn ready() -> bool {
    QUEUE.with(|q| q.try_borrow().is_ok_and(|q| !q.ready.is_empty()))
}

/// The close-callbacks phase.
pub(crate) fn run() {
    let waiting = QUEUE.with(|q| !q.borrow().waiting.is_empty());
    if waiting {
        for completion in
            crate::event_pump::take_closed_completions(is_queued_close, owner_of, retires_with)
        {
            super::dispatch(completion);
        }
    }
    let Some(horizon) = QUEUE.with(|q| q.borrow().next_seq.checked_sub(1)) else {
        return;
    };
    // One taken at a time, with no borrow held: delivery runs JS, which may
    // close more sockets (they wait for the next phase) or deliver a `Closed`.
    while let Some(entry) = QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let seq = *q.ready.range(..=horizon).next_back()?;
        q.ready.remove(&seq);
        q.entries.remove(&seq)
    }) {
        match entry.owner {
            // SAFETY: the entry's ref pins the cell until this call.
            Owner::Cell(cell) => unsafe {
                transport::deliver_closed(
                    OwnerLink(cell),
                    entry.route,
                    entry.handle,
                    entry.released,
                    entry.stale,
                )
            },
            Owner::Id(id) => sink::emit(entry.route, NetCompletion::closed(id)),
        }
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    }
}

/// Drop every queued close (agent teardown, test reset), returning the cells
/// whose refs the caller releases and the id closes already ready, which
/// the caller emits. A waiting id close is emitted by its own `Closed`.
pub(super) fn take_all() -> (Vec<usize>, Vec<(u8, i64)>) {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let ready = std::mem::take(&mut q.ready);
        q.waiting.clear();
        let mut cells = Vec::new();
        let mut ids = Vec::new();
        for (seq, entry) in std::mem::take(&mut q.entries) {
            match entry.owner {
                Owner::Cell(cell) => cells.push(cell),
                Owner::Id(id) if ready.contains(&seq) => ids.push((entry.route, id)),
                Owner::Id(_) => {}
            }
        }
        (cells, ids)
    })
}
