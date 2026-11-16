//! `PERRY_WPF_UNKSRC=1`: the statements that are unknown code by
//! themselves. Each producer of unknown code records (kind, reason, the
//! builtin or host name, call, owner function) once per (kind, reason,
//! call) on a solve-order clock; the first moment `Model::realm_held`
//! becomes true is recorded with its producer. Printed to stderr at the end
//! of the solve, sorted by the clock. Off: one cached flag read per
//! producer; never read by facts.

use std::sync::Mutex;
use std::collections::BTreeSet;

struct Ev {
    seq: u32,
    kind: &'static str,
    reason: String,
    call: u32,
    func: u32,
}

type Held = (u32, &'static str, String, u32, u32, String);

#[derive(Default)]
struct St {
    seq: u32,
    seen: BTreeSet<(&'static str, String, u32)>,
    evs: Vec<Ev>,
    held: Option<Held>,
}

// One store for the process: generation and the solve may run on
// different threads.
static ST: Mutex<Option<St>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut St) -> R) -> R {
    let mut g = ST.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(St::default))
}

pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERRY_WPF_UNKSRC").is_some())
}

/// One producer event; `call`/`func` are `u32::MAX` where none applies.
pub(crate) fn note(kind: &'static str, reason: &str, call: u32, func: u32) {
    with(|s| {
        if s.seen.insert((kind, reason.to_string(), call)) {
            let seq = s.seq;
            s.seq += 1;
            s.evs.push(Ev { seq, kind, reason: reason.to_string(), call, func });
        }
    });
}

/// `realm_held` is about to become true (the first time only).
pub(crate) fn held(kind: &'static str, reason: &str, call: u32, func: u32) {
    with(|s| {
        if s.held.is_none() {
            let seq = s.seq;
            s.seq += 1;
            s.held = Some((seq, kind, reason.to_string(), call, func, String::new()));
        }
    });
}

/// Print and clear (one solve).
pub(crate) fn flush(label: &str, func: impl Fn(u32) -> String) {
    let s = with(std::mem::take);
    {
        if let Some((seq, kind, reason, call, f, _)) = &s.held {
            eprintln!(
                "perry: wpfacts unksrc {label} HELD seq={seq} kind={kind} call={call} fn={} reason={reason}",
                func(*f)
            );
        }
        for e in &s.evs {
            eprintln!(
                "perry: wpfacts unksrc {label} EV seq={} kind={} call={} fn={} reason={}",
                e.seq,
                e.kind,
                e.call,
                func(e.func),
                e.reason
            );
        }
        eprintln!("perry: wpfacts unksrc {label} END events={}", s.evs.len());
    }
}

/// The function whose body holds `call` (`u32::MAX` if none).
pub(crate) fn call_fn(m: &crate::model::Model, call: u32) -> u32 {
    m.calls
        .get(call as usize)
        .and_then(|c| m.node_fn.get(c.ret as usize))
        .map(|&f| f as u32)
        .unwrap_or(u32::MAX)
}
