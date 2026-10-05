//! The zlib stream tables, one set per agent, and their cleanup when a
//! thread exits.

use super::*;

/// One agent's zlib streams: their listeners, pipe destinations and queued
/// events are objects of that agent's heap (the main thread's or a
/// `worker_threads` worker's), so only that agent may pump them. One shared
/// set let the main thread take a worker's events and call its closures on
/// the wrong heap. Same layout as perry-ext-zlib's per-agent state (#11340).
pub(super) struct ZlibTables {
    pub(super) streams: Mutex<HashMap<i64, ZlibStreamState>>,
    pub(super) listeners: Mutex<HashMap<i64, HashMap<String, Vec<i64>>>>,
    pub(super) pending: Mutex<Vec<ZlibEvent>>,
}

static ALL_ZLIB_TABLES: std::sync::LazyLock<Mutex<HashMap<u64, &'static ZlibTables>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Emptied tables of retired agents, handed to the next agent that needs
/// tables. A set is never freed: [`release_zlib_in_freed_ranges`] may hold a
/// copy of its reference while another thread retires its agent. Reuse keeps
/// the leaked sets bounded by the most agents ever alive at once.
static FREE_ZLIB_TABLES: Mutex<Vec<&'static ZlibTables>> = Mutex::new(Vec::new());

thread_local! {
    /// This thread's last (agent, tables) pair, so a hot path skips the map.
    static MINE: std::cell::Cell<Option<(u64, &'static ZlibTables)>> =
        const { std::cell::Cell::new(None) };
}

/// The calling agent's tables, made or reused on its first use and released
/// when the agent retires ([`release_zlib_tables_of_agent`]).
pub(super) fn tables() -> &'static ZlibTables {
    let agent = perry_runtime::agent::current_agent() as u64;
    if let Some((cached, mine)) = MINE.with(std::cell::Cell::get) {
        if cached == agent {
            return mine;
        }
    }
    // Registered before the first entry, so no agent's entry predates it.
    static RETIRE_HOOK: std::sync::Once = std::sync::Once::new();
    RETIRE_HOOK
        .call_once(|| perry_runtime::agent::register_retire_hook(release_zlib_tables_of_agent));
    let mine = *ALL_ZLIB_TABLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(agent)
        .or_insert_with(|| {
            let reused = FREE_ZLIB_TABLES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop();
            reused.unwrap_or_else(|| {
                Box::leak(Box::new(ZlibTables {
                    streams: Mutex::new(HashMap::new()),
                    listeners: Mutex::new(HashMap::new()),
                    pending: Mutex::new(Vec::new()),
                }))
            })
        });
    MINE.with(|slot| slot.set(Some((agent, mine))));
    mine
}

/// `perry_runtime::agent::retire_agent` hook. Only its own agent pumps a set
/// of tables, so a retired agent's streams, listeners and queued events can
/// never run; without this they, and the agent's map entry, would stay for
/// the life of the process. The emptied set goes to [`FREE_ZLIB_TABLES`].
/// The retiring thread forgets its cached pair first: it keeps its dead agent
/// id, so a later `tables()` on it would otherwise return a set that another
/// agent now owns.
pub(super) fn release_zlib_tables_of_agent(agent: perry_runtime::agent::AgentId) {
    use std::sync::PoisonError;
    let _ = MINE.try_with(|slot| {
        if matches!(slot.get(), Some((cached, _)) if cached == agent) {
            slot.set(None);
        }
    });
    let Some(retired) = ALL_ZLIB_TABLES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&agent)
    else {
        return;
    };
    fn take_all<T: Default>(table: &Mutex<T>) -> T {
        std::mem::take(&mut *table.lock().unwrap_or_else(PoisonError::into_inner))
    }
    // Dropped outside the table locks: a codec's state can be large.
    drop((
        take_all(&retired.streams),
        take_all(&retired.listeners),
        take_all(&retired.pending),
    ));
    FREE_ZLIB_TABLES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(retired);
}

/// Test probe: the number of streams agent `agent` holds, or `None` when it
/// holds no tables.
#[cfg(test)]
pub(crate) fn zlib_agent_stream_count_for_test(
    agent: perry_runtime::agent::AgentId,
) -> Option<usize> {
    ALL_ZLIB_TABLES
        .lock()
        .unwrap()
        .get(&agent)
        .map(|tables| tables.streams.lock().unwrap().len())
}

/// #11471: an exiting thread's zlib records outlive its heap: `.pipe(dest)`
/// destinations and `.on(event, cb)` listeners are the setting thread's heap
/// objects, and `.flush(cb)` / `zlib.gzip(data, cb)` queue raw closure
/// pointers that `js_zlib_process_pending` would later call. A thread with no
/// agent of its own fills the primary agent's tables, which outlive it, so
/// their records must not keep pointing at freed (or reused) memory. Every
/// agent's tables are checked: this runs in a TLS destructor, where the agent
/// thread-local may be gone.
///
/// A zlib stream whose pipes or listeners lie in `freed` is dropped whole
/// (state, listeners, and its queued Data/End/Error events); a queued
/// callback event whose closure lies in `freed` is dropped. Handle ids are
/// monotonic (never reused), so no other record can come to name a dropped
/// id. `js_zlib_has_active_handles` reads the queue's emptiness directly, so
/// shrinking it keeps the loop-liveness answer consistent. The dropped
/// streams' async-hooks `destroy` is not emitted (that would run JS).
///
/// Runs in a TLS destructor: one lock at a time, poison-tolerant, no JS heap
/// access.
pub(super) fn release_zlib_in_freed_ranges(freed: &perry_runtime::arena::thread_exit::FreedRanges) {
    let all: Vec<&'static ZlibTables> = ALL_ZLIB_TABLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .copied()
        .collect();
    for tables in all {
        release_zlib_tables_in_freed_ranges(tables, freed);
    }
}

fn release_zlib_tables_in_freed_ranges(
    tables: &ZlibTables,
    freed: &perry_runtime::arena::thread_exit::FreedRanges,
) {
    use std::sync::PoisonError;
    let mut dead: std::collections::HashSet<i64> = std::collections::HashSet::new();
    {
        let g = tables
            .streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for (&id, s) in g.iter() {
            if s.pipes.iter().any(|&bits| freed.holds_bits(bits)) {
                dead.insert(id);
            }
        }
    }
    {
        let g = tables
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for (&id, per_event) in g.iter() {
            if per_event.values().flatten().any(|&cb| freed.holds_i64(cb)) {
                dead.insert(id);
            }
        }
    }
    if !dead.is_empty() {
        tables
            .streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|id, _| !dead.contains(id));
        tables
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|id, _| !dead.contains(id));
    }
    tables
        .pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|ev| match ev {
            ZlibEvent::Data(id, _) | ZlibEvent::End(id) | ZlibEvent::Error(id, _) => {
                !dead.contains(id)
            }
            ZlibEvent::Callback(cb) | ZlibEvent::OneShotCallback(cb, _, _) => !freed.holds_i64(*cb),
        });
}
