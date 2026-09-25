//! Charter step 3: a real-code census of the property-ATTRIBUTE machinery.
//!
//! Armed by `PERRY_ATTR_DIAG=<path|1>`, compiled in only with the
//! `attr-census` feature (every hook is `#[cfg(feature = "attr-census")]`, so a
//! stock build carries none of it). One relaxed load when unarmed.
//!
//! What it answers, for tsc and Zod: which attribute side tables and
//! per-object flags a real program WRITES (installs, by API and by the cell
//! kind of the owner) and which accesses CONSULT them (by API, by call site,
//! by outcome). A table that real code never consults is a deletion with no
//! speed consequence; one consulted on a hot path is where a shape fact pays.
//!
//! Every event is keyed by `(event name, caller file, caller line)`; the call
//! site comes from `#[track_caller]` on the instrumented API functions, which
//! the same feature gates.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::hot_diag::{sink_from_env, write_sink, Sink};

static STATE: AtomicU8 = AtomicU8::new(0);
static SINK: OnceLock<Option<Sink>> = OnceLock::new();

type Key = (&'static str, &'static str, u32);

static TABLE: OnceLock<Mutex<HashMap<Key, u64>>> = OnceLock::new();
static KINDS: OnceLock<Mutex<HashMap<(&'static str, &'static str), u64>>> = OnceLock::new();

fn table() -> &'static Mutex<HashMap<Key, u64>> {
    crate::once_init::get_or_init(&TABLE, || Mutex::new(HashMap::new()))
}

fn kinds() -> &'static Mutex<HashMap<(&'static str, &'static str), u64>> {
    crate::once_init::get_or_init(&KINDS, || Mutex::new(HashMap::new()))
}

/// [`note`] plus a split of the same event by the owner's cell kind.
#[track_caller]
pub(crate) fn note_kind(event: &'static str, owner: usize) {
    if !armed() {
        return;
    }
    note_at(event, std::panic::Location::caller());
    let kind = owner_kind(owner);
    if let Ok(mut t) = kinds().lock() {
        *t.entry((event, kind)).or_insert(0) += 1;
    }
}

fn sink() -> &'static Option<Sink> {
    crate::once_init::get_or_init(&SINK, || sink_from_env("PERRY_ATTR_DIAG"))
}

#[inline]
pub(crate) fn armed() -> bool {
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => resolve_arming(),
    }
}

#[cold]
#[inline(never)]
fn resolve_arming() -> bool {
    let on = sink().is_some();
    STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
    if on {
        register_exit_dump();
    }
    on
}

extern "C" fn exit_dump_shim() {
    dump();
}

fn register_exit_dump() {
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    if REGISTERED.swap(true, Ordering::Relaxed) {
        return;
    }
    unsafe {
        libc::atexit(exit_dump_shim);
    }
}

/// The cell kind of an owner, for install/consult splits. Reads only the GC
/// header; `"nonheap"` for anything without one (handle ids, typed arrays).
pub(crate) fn owner_kind(addr: usize) -> &'static str {
    unsafe {
        let Some(h) = crate::value::addr_class::try_read_gc_header(addr) else {
            return "nonheap";
        };
        match h.obj_type {
            // Only the header byte: asking `object_prototype_addr_matches`
            // here can lazily build Object.prototype, whose builtin installs
            // re-enter this census and recurse until the stack overflows.
            crate::gc::GC_TYPE_OBJECT => "object",
            crate::gc::GC_TYPE_ARRAY => "array",
            crate::gc::GC_TYPE_CLOSURE => "closure",
            crate::gc::GC_TYPE_ERROR => "error",
            crate::gc::GC_TYPE_REGEXP => "regexp",
            _ => "other",
        }
    }
}

/// Record one event at an explicit location.
pub(crate) fn note_at(event: &'static str, loc: &'static std::panic::Location<'static>) {
    if !armed() {
        return;
    }
    let key = (event, loc.file(), loc.line());
    // A periodic snapshot every 2^20 events at one site, for a run that is
    // killed before its exit hook.
    let snapshot = match table().lock() {
        Ok(mut t) => {
            let n = t.entry(key).or_insert(0);
            *n += 1;
            *n & ((1 << 20) - 1) == 0
        }
        Err(_) => false,
    };
    if snapshot {
        dump();
    }
}

/// Record one event at the caller of the `#[track_caller]` function that
/// calls this.
#[track_caller]
#[inline]
pub(crate) fn note(event: &'static str) {
    if !armed() {
        return;
    }
    note_at(event, std::panic::Location::caller());
}

/// Add `n` to an event with no meaningful site (a byte count).
pub(crate) fn note_global_n(event: &'static str, n: u64) {
    if !armed() {
        return;
    }
    let key = (event, "-", 0);
    if let Ok(mut t) = table().lock() {
        *t.entry(key).or_insert(0) += n;
    }
}

/// Record an event with no meaningful site (`"-"`, line 0).
pub(crate) fn note_global(event: &'static str) {
    if !armed() {
        return;
    }
    let key = (event, "-", 0);
    if let Ok(mut t) = table().lock() {
        *t.entry(key).or_insert(0) += 1;
    }
}

fn dump() {
    let Some(sink) = sink().as_ref() else {
        return;
    };
    let Ok(t) = table().lock() else {
        return;
    };
    let mut by_event: HashMap<&'static str, u64> = HashMap::new();
    for ((event, _, _), n) in t.iter() {
        *by_event.entry(*event).or_insert(0) += *n;
    }
    let mut events: Vec<_> = by_event.into_iter().collect();
    events.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let mut out = String::new();
    out.push_str("# perry attr census (charter step 3)\n## totals by event\n");
    for (event, n) in &events {
        out.push_str(&format!("{n:>12} {event}\n"));
    }
    if let Ok(k) = kinds().lock() {
        out.push_str("## by event and owner kind\n");
        let mut rows: Vec<_> = k.iter().collect();
        rows.sort_by(|a, b| a.0 .0.cmp(b.0 .0).then(b.1.cmp(a.1)));
        for ((event, kind), n) in rows {
            out.push_str(&format!("{n:>12} {event} [{kind}]\n"));
        }
    }
    out.push_str("## by event and call site\n");
    let mut rows: Vec<_> = t.iter().collect();
    rows.sort_by(|a, b| a.0 .0.cmp(b.0 .0).then(b.1.cmp(a.1)));
    for ((event, file, line), n) in rows {
        out.push_str(&format!("{n:>12} {event} {file}:{line}\n"));
    }
    out.push_str("ATTR_CENSUS_END\n");
    write_sink(sink, &out);
}
