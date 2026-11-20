//! An EOF read behind data waits for the check phase, as libuv's does.
//!
//! libuv stops reading a stream after a short read, so a peer's `end("a")`
//! reaches Node as two polls: the data in one, the EOF in the next. Between
//! them that iteration's check phase runs, so a `setImmediate` scheduled from
//! the `'data'` handler runs before `'end'` and before the `allowHalfOpen:
//! false` auto-end it queues. turnloop reports both results from one turn,
//! and delivering them back to back ended the writable side first: the
//! immediate's `socket.end(x)` then failed with `ERR_STREAM_WRITE_AFTER_END`.
//!
//! The rule: an EOF whose socket received data since the current check phase
//! began is parked, and delivered at the end of the next check phase to begin
//! — after every immediate the data handler could have queued. An EOF with no
//! data before it (a plain close) is delivered at once.
//!
//! Only the *delivery* waits. The driver's read state is cleared when the EOF
//! arrives, so nothing is resubmitted. A parked EOF is dropped, never
//! delivered late, when its socket closes or is replaced first: the flush
//! re-validates the entry (id route) or the core's incarnation (link route,
//! whose cell the parked EOF pins with a resource ref until then), and a
//! `Closed` purges the socket's parked EOF with it.
//!
//! `PERRY_NET_DEFER_EOF=0` delivers every EOF at once (the old order).

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU8, Ordering};

use turnloop::Handle;

use super::*;
use crate::native_payload::OwnerLink;

/// Which socket an EOF belongs to: an id-route entry or a link-route cell.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Key {
    Id(i64),
    Link(usize),
}

struct Parked {
    key: Key,
    /// Id route: the subsystem. Link route: unused (the core's route is read
    /// at delivery, since a rerouted socket's EOF belongs to its new sink).
    subsystem: u8,
    /// The incarnation that read the EOF.
    handle: Option<Handle>,
    /// The check phase in progress (or last begun) when the EOF arrived.
    phase: u64,
}

#[derive(Default)]
struct DeferState {
    /// Check phases begun on this thread.
    phase: u64,
    /// `fed` lists the sockets that received data during check phase `fed_phase`.
    fed_phase: u64,
    fed: HashSet<Key>,
    parked: VecDeque<Parked>,
}

crate::perry_thread_local! {
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_tlnet")]
    static DEFER: RefCell<DeferState> = RefCell::new(DeferState::default());
}

const UNRESOLVED: u8 = 0;
const ON: u8 = 1;
const OFF: u8 = 2;
static ENABLED: AtomicU8 = AtomicU8::new(UNRESOLVED);

fn enabled() -> bool {
    match ENABLED.load(Ordering::Relaxed) {
        ON => true,
        OFF => false,
        _ => {
            let on = crate::gc::env_default_on_enabled("PERRY_NET_DEFER_EOF");
            ENABLED.store(if on { ON } else { OFF }, Ordering::Relaxed);
            on
        }
    }
}

#[cfg(test)]
pub(crate) fn set_enabled_for_test(on: bool) {
    ENABLED.store(if on { ON } else { OFF }, Ordering::Relaxed);
}

/// A read completion carried data for `key`.
pub(super) fn note_data(key: Key) {
    if !enabled() {
        return;
    }
    DEFER.with(|d| {
        let mut d = d.borrow_mut();
        if d.fed_phase != d.phase {
            d.fed_phase = d.phase;
            d.fed.clear();
        }
        d.fed.insert(key);
    });
}

/// Park the EOF just read for `key` if data reached it since the current
/// check phase began. `true` means it is parked and the caller must not
/// deliver it; a link-route caller then holds a resource ref for it.
pub(super) fn park(key: Key, subsystem: u8, handle: Option<Handle>) -> bool {
    if !enabled() {
        return false;
    }
    DEFER.with(|d| {
        let mut d = d.borrow_mut();
        if d.fed_phase != d.phase || !d.fed.contains(&key) {
            return false;
        }
        let phase = d.phase;
        d.parked.push_back(Parked {
            key,
            subsystem,
            handle,
            phase,
        });
        true
    })
}

/// Whether an EOF waits for a check phase (the park must not block on it).
pub(crate) fn pending() -> bool {
    DEFER.with(|d| d.try_borrow().is_ok_and(|d| !d.parked.is_empty()))
}

/// The check phase begins.
pub(crate) fn check_phase_begin() {
    DEFER.with(|d| d.borrow_mut().phase += 1);
}

/// The check phase ended: deliver every EOF parked before it began, in
/// arrival order. One taken at a time, with no borrow held, because delivery
/// runs the binding's sink (JS), which may read, close or park more.
pub(crate) fn check_phase_end() {
    let now = DEFER.with(|d| d.borrow().phase);
    while let Some(parked) = DEFER.with(|d| {
        let mut d = d.borrow_mut();
        let due = d.parked.front().is_some_and(|p| p.phase < now);
        due.then(|| d.parked.pop_front()).flatten()
    }) {
        deliver(parked);
    }
}

fn deliver(parked: Parked) {
    match parked.key {
        Key::Id(id) => {
            let live = NET.with(|net| {
                net.borrow()
                    .entries
                    .get(&id)
                    .is_some_and(|entry| !entry.closing && Some(entry.handle) == parked.handle)
            });
            if live {
                sink::emit(parked.subsystem, NetCompletion::eof(id));
            }
        }
        // SAFETY: the parked EOF's ref pins the cell until this call.
        Key::Link(cell) => unsafe { transport::deliver_parked_eof(OwnerLink(cell), parked.handle) },
    }
}

/// Drop `key`'s parked EOF: its socket closed. A link cell outlives its
/// incarnations, so there only the EOF read by `handle` goes. Returns how
/// many were dropped (a link-route caller releases one ref for each).
pub(super) fn purge(key: Key, handle: Option<Handle>) -> usize {
    DEFER.with(|d| {
        let mut d = d.borrow_mut();
        let before = d.parked.len();
        d.parked
            .retain(|p| p.key != key || matches!(key, Key::Link(_)) && p.handle != handle);
        before - d.parked.len()
    })
}

/// Drop every parked EOF (agent teardown, test reset), returning the link
/// cells whose refs the caller releases.
pub(super) fn take_all() -> Vec<usize> {
    DEFER.with(|d| {
        let mut d = d.borrow_mut();
        d.fed.clear();
        d.parked
            .drain(..)
            .filter_map(|p| match p.key {
                Key::Link(cell) => Some(cell),
                Key::Id(_) => None,
            })
            .collect()
    })
}
