//! The per-socket write queue: which caller writes go to the driver now, and
//! which wait in the socket's [`Backlog`] (perry#11106).
//!
//! A socket has at most [`MAX_INFLIGHT_WRITES`] driver write operations
//! outstanding. Anything written while that many are in flight, or while the
//! connect has not completed, is appended to the backlog; each write
//! completion then submits the whole backlog as ONE buffer. The caller still
//! sees one `Wrote` completion per `write()` it made, in order, with its own
//! length and token — the coalescing is invisible above this module except in
//! how many operations it costs the loop.

use super::*;

/// Caller writes the driver refused while a backlog was being flushed. Built
/// under the `NET` borrow and reported once it has been released, because the
/// sink re-enters this module.
pub(super) struct FlushFailure {
    error: NodeError,
    users: Vec<u64>,
}

/// Bytes accepted from the caller and not yet reported written: submitted to
/// the driver plus still waiting in the backlog. Node's `writableLength`.
pub(super) fn total_queued(net: &NetState, id: i64) -> usize {
    net.entries.get(&id).map_or(0, |e| e.queued)
        + net.backlogs.get(&id).map_or(0, |b| b.bytes.len())
}

fn invalid(syscall: &'static str) -> NodeError {
    map_error(Error::new(ErrorKind::InvalidInput), syscall)
}

/// Submit `bytes` as one driver write. The caller records the
/// [`PendingWrite`]s it covers.
fn submit_bytes(
    driver: &mut turnloop::Loop,
    id: i64,
    entry: &mut Entry,
    bytes: Vec<u8>,
) -> Result<(), Error> {
    let len = bytes.len();
    driver.write(entry.handle, WriteBuf::Owned(bytes), token(OP_WRITE, id))?;
    census::note_submit(OP_WRITE);
    entry.queued += len;
    entry.inflight += 1;
    Ok(())
}

fn submit_shutdown(
    driver: &mut turnloop::Loop,
    id: i64,
    entry: &mut Entry,
    user: u64,
) -> Result<(), Error> {
    driver.shutdown(entry.handle, token(OP_SHUTDOWN, id))?;
    census::note_submit(OP_SHUTDOWN);
    // The user token rides the pending-write queue's tail slot so the
    // `Shutdown` completion can echo it back; a zero-length entry never
    // affects `queued`.
    entry.writes.push_back(PendingWrite {
        user,
        len: 0,
        last: true,
    });
    Ok(())
}

/// Accept one caller write: straight to the driver when nothing is ahead of
/// it, otherwise into the backlog. Returns the socket's total queued bytes.
pub(super) fn accept_write(
    driver: &mut turnloop::Loop,
    net: &mut NetState,
    id: i64,
    bytes: Vec<u8>,
    user: u64,
) -> NetResult<usize> {
    let NetState {
        entries,
        plans,
        backlogs,
        ..
    } = &mut *net;
    // Between two attempts of a connect plan the failed attempt's entry is
    // still there, closing, until its `Closed` starts the next address. A
    // write in that window belongs to the attempt that follows, not to the
    // handle being torn down (it used to be refused, which reported
    // `'error'` and destroyed a socket that was about to connect).
    let retrying = plans.get(&id).is_some_and(|p| p.retrying);
    match entries.get_mut(&id) {
        Some(entry) if !retrying => {
            if entry.listener || entry.closing {
                return Err(invalid("write"));
            }
            let waiting = backlogs.get(&id).is_some_and(|b| !b.is_idle());
            if !entry.connecting && !waiting && entry.inflight < MAX_INFLIGHT_WRITES {
                let len = bytes.len();
                submit_bytes(driver, id, entry, bytes).map_err(|e| map_error(e, "write"))?;
                entry.writes.push_back(PendingWrite {
                    user,
                    len,
                    last: true,
                });
            } else {
                let backlog = backlogs.entry(id).or_default();
                if backlog.shutdown.is_some() {
                    return Err(invalid("write"));
                }
                backlog.push(bytes, user);
            }
        }
        // Still resolving (no handle yet), or between attempts.
        _ if plans.contains_key(&id) => {
            let backlog = backlogs.entry(id).or_default();
            if backlog.shutdown.is_some() {
                return Err(invalid("write"));
            }
            backlog.push(bytes, user);
        }
        // No entry and no plan (`retrying` implies a plan, so never `Some`).
        _ => return Err(not_found("write")),
    }
    Ok(total_queued(net, id))
}

/// Accept `end()`: the shutdown is queued behind every write accepted before
/// it, which on a connecting socket means waiting for `Connected`.
pub(super) fn accept_shutdown(
    driver: &mut turnloop::Loop,
    net: &mut NetState,
    id: i64,
    user: u64,
) -> NetResult<()> {
    let retrying = net.plans.get(&id).is_some_and(|p| p.retrying);
    let deferred = match net.entries.get(&id) {
        // Between attempts: see `accept_write`.
        Some(_) if retrying => true,
        Some(entry) if entry.listener || entry.closing => return Err(invalid("shutdown")),
        Some(entry) => entry.connecting,
        None if net.plans.contains_key(&id) => true,
        None => return Err(not_found("shutdown")),
    };
    let backlog = net.backlogs.entry(id).or_default();
    if backlog.shutdown.is_some() {
        // A second shutdown while the first still waits would replace its
        // token and strand that caller's callback; the binding coalesces
        // repeated `end()` calls before they get here.
        return Err(invalid("shutdown"));
    }
    backlog.shutdown = Some(user);
    if deferred {
        return Ok(());
    }
    // `force`: the shutdown must go out behind the backlog now, whatever is
    // in flight — turnloop orders a handle's writes before its shutdown.
    match flush(driver, net, id, true) {
        Some(failure) => Err(failure.error),
        None => Ok(()),
    }
}

/// Hand the backlog (and a deferred shutdown) to the driver if the socket can
/// take it now. `force` ignores the in-flight cap: an `end()`, or a connection
/// that just completed with writes waiting.
pub(super) fn flush(
    driver: &mut turnloop::Loop,
    net: &mut NetState,
    id: i64,
    force: bool,
) -> Option<FlushFailure> {
    let NetState {
        entries, backlogs, ..
    } = &mut *net;
    let entry = entries.get_mut(&id)?;
    if entry.connecting || entry.closing {
        return None;
    }
    let backlog = backlogs.get_mut(&id)?;
    let mut failure = None;
    if !backlog.writes.is_empty() {
        if !force && entry.inflight >= MAX_INFLIGHT_WRITES {
            return None;
        }
        let bytes = std::mem::take(&mut backlog.bytes);
        let mut writes = std::mem::take(&mut backlog.writes);
        match submit_bytes(driver, id, entry, bytes) {
            Ok(()) => {
                if let Some(tail) = writes.last_mut() {
                    tail.last = true;
                }
                entry.writes.extend(writes);
            }
            Err(e) => {
                let mut users: Vec<u64> = writes.iter().map(|w| w.user).collect();
                users.extend(backlog.shutdown.take());
                failure = Some(FlushFailure {
                    error: map_error(e, "write"),
                    users,
                });
            }
        }
    }
    if failure.is_none() {
        if let Some(user) = backlog.shutdown.take() {
            if let Err(e) = submit_shutdown(driver, id, entry, user) {
                failure = Some(FlushFailure {
                    error: map_error(e, "shutdown"),
                    users: vec![user],
                });
            }
        }
    }
    if backlog.is_idle() {
        backlogs.remove(&id);
    }
    failure
}

/// [`flush`] from a completion, with the driver and the `NET` borrow taken
/// here and released before anything is reported.
pub(super) fn flush_after(subsystem: u8, id: i64, force: bool) {
    let failure =
        with_driver(|driver| NET.with(|net| flush(driver, &mut net.borrow_mut(), id, force)))
            .flatten();
    if let Some(failure) = failure {
        report_failure(subsystem, id, failure);
    }
}

/// Retire the caller writes one driver write (or shutdown) covered, oldest
/// first. Each comes back with the socket's total queued bytes after it, so
/// the caller can report `writableLength` exactly as it stood.
pub(super) fn retire(net: &mut NetState, id: i64, op_class: u64) -> Vec<(u64, usize, usize)> {
    let waiting = net.backlogs.get(&id).map_or(0, |b| b.bytes.len());
    let Some(entry) = net.entries.get_mut(&id) else {
        return Vec::new();
    };
    if op_class == OP_WRITE {
        entry.inflight = entry.inflight.saturating_sub(1);
    }
    let mut retired = Vec::new();
    while let Some(w) = entry.writes.pop_front() {
        entry.queued = entry.queued.saturating_sub(w.len);
        retired.push((w.user, w.len, entry.queued + waiting));
        if w.last {
            break;
        }
    }
    retired
}

/// Report a refused flush the way a failed write is reported: every covered
/// write callback gets the error, and the binding sees at least one `'error'`.
pub(super) fn report_failure(subsystem: u8, id: i64, failure: FlushFailure) {
    let queued = NET.with(|net| total_queued(&net.borrow(), id));
    report_error(subsystem, id, &failure.users, queued, failure.error, false);
}

/// One error completion per covered write that has a callback, or a single
/// one with no token when none of them had one.
pub(super) fn report_error(
    subsystem: u8,
    id: i64,
    users: &[u64],
    queued: usize,
    error: NodeError,
    terminal: bool,
) {
    let mut reported = false;
    for &user in users.iter().filter(|&&user| user != 0) {
        sink::emit(
            subsystem,
            NetCompletion::error(id, user, queued, error, terminal),
        );
        reported = true;
    }
    if !reported {
        sink::emit(
            subsystem,
            NetCompletion::error(id, 0, queued, error, terminal),
        );
    }
}
