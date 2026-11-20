//! Existing interpreter prototype-write validity guard, unchanged.
/// Decision 150: interpreted code wrote a prototype the whole-program facts
/// cannot see it write. Set-only, process-wide, `0` until then.
///
/// The facts (perry-wpfacts) model interpreted code as the ⊤ summary node:
/// every value it can receive already escapes. What that model cannot see is
/// interpreted code writing a PROTOTYPE that program objects inherit from
/// (`Object.prototype`, `Array.prototype` …, or any object marked as a
/// prototype): an accessor or method it installs there runs with the
/// receiver of every lookup that reaches it, and a setter there keeps a
/// constructor's `this.f = v` from defining the birth field `f`. So when the
/// program can run interpreted code, every elided field access whose proof
/// rests on the facts tests this word (one load) and takes its complete
/// generic operation once it is set (`perry-codegen` `wpfacts_access.rs`).
/// Writes by compiled code are the facts' own (E5/E6/E20/E23) and do not set
/// it.
///
/// Lives here (always compiled), not in `crate::dyn_eval`: the funnels and
/// generated code reference it in every build.
///
/// Set from every prototype mutation funnel: [`note_proto_write`] is called
/// by the sealed-intrinsic write funnel (`builtin_proto::note_family_write`:
/// slot and spill stores, key adds, deletes, descriptor and attribute
/// changes, relinks of a family prototype) and by the marked-prototype
/// funnels (`proto_validity::note_object_shape_stamped` /
/// `note_object_own_keys_changed`, `builtin_proto::note_prototype_value_store`).
#[no_mangle]
pub static PERRY_INTERP_PROTO_WRITTEN: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

/// A prototype is being written: set [`PERRY_INTERP_PROTO_WRITTEN`] when
/// interpreted code is on this thread's stack.
#[inline]
pub(crate) fn note_proto_write() {
    use std::sync::atomic::Ordering;
    if PERRY_INTERP_PROTO_WRITTEN.load(Ordering::Relaxed) != 0 {
        return;
    }
    if INTERP_ACTIVE.with(|c| c.get()) != 0 {
        PERRY_INTERP_PROTO_WRITTEN.store(1, Ordering::Relaxed);
    }
}

crate::perry_thread_local! {
    /// Entries into interpreted code now on this thread's stack
    /// (`dyn_eval::interp_entry`): non-zero while interpreted code, or
    /// anything it called, runs.
    static INTERP_ACTIVE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// `dyn_eval::interp_entry` entered interpreted code.
#[cfg_attr(not(feature = "dyn-eval"), allow(dead_code))]
pub(crate) fn interp_active_enter() {
    INTERP_ACTIVE.with(|c| c.set(c.get() + 1));
}

/// `dyn_eval::interp_entry` left it (returned or threw).
#[cfg_attr(not(feature = "dyn-eval"), allow(dead_code))]
pub(crate) fn interp_active_leave() {
    INTERP_ACTIVE.with(|c| c.set(c.get().saturating_sub(1)));
}
