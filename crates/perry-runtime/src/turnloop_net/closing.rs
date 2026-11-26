//! The close of a link socket closed while its name was still resolving.
//!
//! Node creates the TCP handle before the lookup, so such a socket's `'close'`
//! comes from the handle's close callback, in the close-callbacks phase: after
//! the ticks and the immediates queued before it. Here there is no handle to
//! close, so [`transport::close`] queues the socket's `Closed` instead, and the
//! end of the check phase in progress, or of the next one to begin, delivers
//! it. Each queued cell holds a resource ref until then.

use std::cell::RefCell;
use std::collections::VecDeque;

use super::*;
use crate::native_payload::OwnerLink;

crate::perry_thread_local! {
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_tlnet")]
    static QUEUED: RefCell<VecDeque<(usize, u8)>> = RefCell::new(VecDeque::new());
}

/// Owe `link`'s `Closed` on `route` to the close-callbacks phase; the caller
/// holds the ref that pins the cell.
pub(super) fn queue(link: OwnerLink, route: u8) {
    QUEUED.with(|q| q.borrow_mut().push_back((link.0, route)));
}

/// Whether a `Closed` waits for the close-callbacks phase.
pub(crate) fn pending() -> bool {
    QUEUED.with(|q| q.try_borrow().is_ok_and(|q| !q.is_empty()))
}

/// The close-callbacks phase: deliver every queued `Closed`. One taken at a
/// time, with no borrow held, because delivery runs the binding's sink (JS).
pub(crate) fn run() {
    while let Some((cell, route)) = QUEUED.with(|q| q.borrow_mut().pop_front()) {
        // SAFETY: the queued ref pins the cell until this call.
        unsafe { transport::deliver_queued_closed(OwnerLink(cell), route) };
    }
}

/// Drop every queued `Closed` (agent teardown, test reset), returning the
/// cells whose refs the caller releases.
pub(super) fn take_all() -> Vec<usize> {
    QUEUED.with(|q| q.borrow_mut().drain(..).map(|(cell, _)| cell).collect())
}
