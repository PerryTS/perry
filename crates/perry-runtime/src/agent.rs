//! Agent (JS heap owner) identity for cross-thread queue dispatch. Issue #6185
//! Tier 2.
//!
//! ## The problem this solves
//!
//! Perry's cross-thread plumbing keeps three process-global queues —
//! `PENDING_THREAD_RESULTS` (`thread.rs`), `TIMER_QUEUE` / `CALLBACK_TIMERS` /
//! `INTERVAL_TIMERS` (`timer.rs`) — whose entries carry raw pointers
//! (`*mut Promise`, closure handles, argument `f64`s that may be NaN-boxed
//! pointers) into *some* thread's arena. Their drains were unconditional: any
//! thread that ran the await loop / microtask pump drained *everything*. A
//! `perry/thread` worker doing async work could therefore steal a main-thread
//! completion, deserialize it into the worker's arena, resolve a main-heap
//! promise with a worker-heap pointer, and fire main-heap timer closures on the
//! worker. When the worker exits, its arena is unmapped and the main thread
//! reads freed memory. The `unsafe impl Send` on those queue entries carried a
//! "only accessed from the pump thread" comment that nothing enforced.
//!
//! ## Why not tag with `ThreadId`
//!
//! The obvious fix — tag each entry with the `ThreadId` that enqueued it and
//! have drains skip entries they don't own — breaks Android. There, the
//! compiled TypeScript (and therefore every `setTimeout` / `setInterval` call)
//! runs on the `perry-native` thread, but the timer pump fires from the UI
//! thread via `nativePumpTick` (`perry-ui-android/src/app.rs`). A blanket
//! owner-skip would leave every Android timer un-fired forever. That's the
//! blocker that deferred the narrow slice of this fix earlier.
//!
//! ## The model: agents, and pumps that act on their behalf
//!
//! The unit that matters is not the OS thread but the **heap** — the arena the
//! queued pointers live in. We call it an *agent*.
//!
//! - The program starts with one agent, [`PRIMARY_AGENT`]. Every thread that
//!   runs user JS on the primary heap (the process main thread; Android's
//!   `perry-native` thread) belongs to it.
//! - Each `perry/thread` worker gets its **own** agent id at thread entry (see
//!   [`enter_worker_agent`]), because it gets its own arena and GC.
//! - A thread that has *not* been given an agent of its own is, by definition,
//!   not a JS heap owner: it is a **pump** running on behalf of the primary
//!   agent. Android's UI thread is exactly this. So [`current_agent`] falls back
//!   to [`PRIMARY_AGENT`] rather than inventing an id.
//!
//! That fallback is what makes the Android path keep working unchanged: the UI
//! thread resolves to `PRIMARY_AGENT`, which is precisely the agent whose timers
//! `perry-native` scheduled. Meanwhile a worker resolves to its own id, so its
//! drains can only ever touch entries it enqueued itself.
//!
//! Queue entries are tagged with [`current_agent`] at *enqueue* time and drains
//! filter on [`current_agent`] at *drain* time. Foreign entries are left in
//! place for their owner (never silently dropped mid-flight); entries belonging
//! to an agent that has exited are purged by [`retire_agent`], since that
//! agent's arena is gone and nothing can ever settle them.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Identity of a JS heap (arena + GC) that queued cross-thread work.
pub type AgentId = u64;

/// The agent that owns the process's initial JS heap: the main thread, and on
/// Android the `perry-native` thread. Threads with no agent of their own (pump
/// threads such as Android's UI thread) act on this agent's behalf.
pub const PRIMARY_AGENT: AgentId = 0;

/// Source of worker agent ids. Starts at 1 — 0 is reserved for
/// [`PRIMARY_AGENT`], which is never handed out here.
static NEXT_AGENT: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The agent this thread *owns*. `None` means "no heap of my own" — the
    /// main/`perry-native` JS thread (which owns the primary heap implicitly)
    /// and any pure pump thread both leave this unset, and so resolve to
    /// [`PRIMARY_AGENT`] via [`current_agent`].
    static CURRENT_AGENT: Cell<Option<AgentId>> = const { Cell::new(None) };
}

/// Claim a fresh agent id for the calling thread. Called once at the top of
/// every `perry/thread` worker (`spawn` / `parallelMap` / `parallelFilter`),
/// before the worker can allocate or enqueue anything, so every pointer it
/// puts into a global queue is tagged as its own.
///
/// Returns the new id so the caller can hand it to [`retire_agent`] at exit.
pub fn enter_worker_agent() -> AgentId {
    let id = NEXT_AGENT.fetch_add(1, Ordering::Relaxed);
    CURRENT_AGENT.with(|slot| slot.set(Some(id)));
    id
}

/// The agent whose queued work the calling thread may touch: its own if it is a
/// worker, otherwise [`PRIMARY_AGENT`] (it is a JS thread on the primary heap,
/// or a pump acting for it).
pub fn current_agent() -> AgentId {
    CURRENT_AGENT
        .with(|slot| slot.get())
        .unwrap_or(PRIMARY_AGENT)
}

/// True when `owner` is work the calling thread is allowed to drain and run.
#[inline]
pub fn owns(owner: AgentId) -> bool {
    owner == current_agent()
}

/// Retire a worker agent at thread exit: its arena is about to be unmapped, so
/// any queue entry still tagged with it points at memory that is about to go
/// away. Purge those entries rather than leaving them for a drain that can
/// never legally run.
///
/// Purging is safe precisely because nothing else can own them: the entries name
/// pointers into *this* agent's arena, and no other thread may dereference them.
pub fn retire_agent(id: AgentId) {
    debug_assert_ne!(
        id, PRIMARY_AGENT,
        "the primary agent outlives the process; it is never retired"
    );
    crate::timer::purge_agent_timers(id);
    crate::thread::purge_agent_thread_results(id);
    // Deliberately do NOT clear `CURRENT_AGENT`. Clearing it would make
    // `current_agent()` fall back to `PRIMARY_AGENT` for the rest of this
    // thread's life — i.e. a worker that has just torn down its heap would
    // start reporting itself as an owner of the *main* thread's queued work,
    // reintroducing exactly the cross-heap drain this fix removes. A retired
    // worker keeps its (now dead) id and therefore owns nothing at all.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain thread that never claims a worker agent — the main thread, or a
    /// pump like Android's UI thread — resolves to the primary agent, so it
    /// keeps draining exactly the work the primary JS thread scheduled. This is
    /// the property that keeps `nativePumpTick` delivering Android timers.
    #[test]
    fn unclaimed_threads_pump_for_the_primary_agent() {
        assert_eq!(current_agent(), PRIMARY_AGENT);
        assert!(owns(PRIMARY_AGENT));

        let pump_thread_agent = std::thread::spawn(current_agent).join().unwrap();
        assert_eq!(pump_thread_agent, PRIMARY_AGENT);
    }

    /// A worker claims its own agent, so it neither drains the primary agent's
    /// work nor collides with a sibling worker.
    #[test]
    fn workers_get_distinct_agents_and_disown_primary_work() {
        let a = std::thread::spawn(|| {
            let id = enter_worker_agent();
            assert_eq!(current_agent(), id);
            assert!(!owns(PRIMARY_AGENT), "a worker must not own primary work");
            id
        })
        .join()
        .unwrap();

        let b = std::thread::spawn(|| enter_worker_agent()).join().unwrap();

        assert_ne!(a, PRIMARY_AGENT);
        assert_ne!(b, PRIMARY_AGENT);
        assert_ne!(a, b, "sibling workers must not share an agent");
    }
}
