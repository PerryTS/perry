//! Mark-sweep garbage collector for Perry
//!
//! Design:
//! - 8-byte GcHeader prepended to every heap allocation (invisible to callers)
//! - Arena objects (arrays/objects): discovered by walking arena blocks linearly (zero per-alloc tracking cost)
//! - Explicit malloc objects (promises/maps/errors, large closures, and compatibility residents): tracked in MALLOC_STATE
//! - Mark phase: precise thread-local roots + optional conservative stack scan + type-specific tracing
//! - Sweep phase: free malloc objects; arena objects added to free list for reuse
//! - Trigger: only checked on new arena block allocation or explicit gc() call
//!
//! Low-pause contract:
//! - Normal automatic GC work and mutator assists must eventually advance in
//!   bounded work-unit steps, independent of heap size.
//! - Explicit `gc()` calls may synchronously run the configured collection
//!   because the caller requested that pause; traces distinguish manual minor
//!   work from explicit full collection.
//! - Emergency full collections are reserved for allocation failure recovery,
//!   only outside suppressed, reentrant, or unsafe regions, and must be
//!   reported separately.
//!
//! Threshold-triggered work in `gc_check_trigger()` is debt-paced: heap goals
//! start or resume a budgeted cycle and allocation-side checks spend bounded
//! mutator-assist work instead of running a whole automatic collection.

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex, MutexGuard, OnceLock,
};
use std::time::{Duration, Instant};

mod types;
pub use types::*;
mod policy;
pub(crate) use policy::gc_runtime_safepoint;
pub use policy::*;
mod progress;
pub use progress::*;
mod heap_budget;
pub use heap_budget::*;
mod pressure;
pub use pressure::*;
mod telemetry;
pub use telemetry::*;
mod malloc;
pub use malloc::*;
mod roots;
pub use roots::*;
mod layout;
pub use layout::*;
mod trace;
pub(crate) use trace::*;
mod barrier;
pub use barrier::*;
mod copying;
use copying::*;
// The copied-minor pointer classifier is consumed by the weak-holder registry
// pass in `crate::weakref` (#6182), which lives outside the gc module.
pub(crate) use copying::{CopyingPointer, CopyingPointerSet};
mod dead_owner;
mod oldgen;
use oldgen::*;
mod cycle;
use cycle::*;
mod verify;
pub use verify::*;
#[cfg(feature = "diagnostics")]
mod heap_snapshot;
#[cfg(feature = "diagnostics")]
pub use heap_snapshot::gc_build_v8_heap_snapshot_json;

pub fn gc_collect_minor() -> u64 {
    if defer_gc_request(DeferredGcRequest::DirectMinor) {
        return 0;
    }
    gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current()
}

pub(super) fn gc_collect_minor_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    gc_drain_active_budgeted_cycle();
    // Barriers-off ⇒ the remembered set is not being maintained, and a
    // minor's black-leafed old parents would hide live children. Route
    // every caller (direct arm, moving-safepoint arm, public FFI) to the
    // full collection instead of trusting an empty RS.
    if !gen_gc_enabled() {
        return gc_collect_full_mark_sweep_with_trigger(trigger);
    }
    // Phase C4b-γ-3: re-entrancy guard. Without this, the evacuation
    // pass's `arena_alloc_gc_old` can trigger `gc_check_trigger` (via
    // `arena.alloc`'s slow-path block-fill) DURING the outer collection
    // cycle. The outer cycle's MARK_SEEDS, CONS_PINNED, and valid_ptrs
    // are all in indeterminate states mid-evac; a recursive
    // `gc_collect_minor` clears them, runs its own mark phase from a
    // mostly-empty C-stack snapshot (we're deep inside the runtime,
    // very few user pointers reachable), evacuates whatever it can find,
    // then returns to the outer cycle which proceeds with corrupt
    // pinning + corrupt seed list. Symptom: bench_evac_heavy's `cache`
    // local gets evacuated by the inner cycle (un-pinned because the
    // inner mark_stack_roots can't see it through the deep-runtime
    // stack), and the outer rewrite walk doesn't update the user's
    // shadow stack slot to point at the new copy → cache.length reads
    // garbage from the FORWARDED slot's first 8 bytes thereafter.
    //
    // Fix: set GC_FLAG_IN_ALLOC for the entire duration of
    // gc_collect_minor. `gc_check_trigger` already early-returns when
    // this bit is set. Any recursive `gc_check_trigger` call from
    // arena_alloc_gc_old / arena_alloc_gc / gc_malloc inside the
    // collection sees the bit and bails. The outer cycle's bookkeeping
    // stays intact.
    let prev_in_alloc = GC_FLAGS.with(|f| {
        let prev = f.get();
        f.set(prev | GC_FLAG_IN_ALLOC);
        prev & GC_FLAG_IN_ALLOC
    });
    if copied_minor_promotion_handoff_due(trigger.kind) {
        let outcome = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
            GcTriggerKind::SurvivorPromotionBytes,
        ));
        restore_minor_in_alloc(prev_in_alloc);
        return outcome;
    }
    let mut trace = GcCycleTrace::new(GcCollectionKind::Minor, trigger);
    let start = Instant::now();
    crate::arena::old_pages_begin_gc_cycle();
    let previous_pause_us = gc_last_pause_us();
    let current_rss_bytes = crate::process::get_rss_bytes();
    let evacuation_policy_allowed = gen_gc_evacuate_enabled();
    let force_evacuation = gc_force_evacuate_enabled();
    let old_page_selection = if evacuation_policy_allowed && old_to_young_tracking_complete() {
        select_old_page_defrag_pages(force_evacuation)
    } else {
        OldPageDefragSelection::default()
    };
    let old_page_source_blocks =
        crate::arena::old_arena_source_blocks_for_pages(&old_page_selection.pages);
    // MARK_SEEDS persists across GC cycles. Clear before any try_mark
    // call so trace sees only this cycle's freshly-marked headers.
    clear_mark_seeds();
    if let Some(fast_path) = gc_collect_minor_copying_fast_path(&mut trace, start, trigger.kind) {
        let freed_bytes = fast_path.freed_bytes;
        let elapsed_us = start.elapsed().as_micros() as u64;
        GC_STATS.with(|stats| {
            stats
                .borrow_mut()
                .record_collection(freed_bytes, elapsed_us);
        });
        restore_minor_in_alloc(prev_in_alloc);
        if let Some(trace) = trace.as_mut() {
            trace.pause_us = elapsed_us;
            trace.capture_layout_scans();
        }
        return GcCollectOutcome {
            freed_bytes,
            malloc_swept: fast_path.malloc_swept,
            trace,
        };
    }
    clear_mark_seeds();
    GcCycleState::new_minor_fallback(
        trigger,
        trace,
        start,
        trigger.kind.progress_kind(GcCollectionKind::Minor),
        prev_in_alloc,
        previous_pause_us,
        current_rss_bytes,
        evacuation_policy_allowed,
        force_evacuation,
        EVACUATION_POLICY_DISABLED_REASON,
        old_page_selection,
        old_page_source_blocks,
    )
    .run_to_completion()
}

#[inline]

pub fn gen_gc_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        // Generational minors are only sound with runtime write barriers:
        // minors black-leaf old parents and trust the remembered set for
        // every old→young/old→malloc edge. `PERRY_WRITE_BARRIERS=0` used
        // to disable only evacuation gating while minors kept running —
        // the remembered set stayed empty, so a born-old (>16 KB) parent's
        // nursery children were swept on the first minor and the
        // "bisection" mode crashed for reasons unrelated to what was being
        // bisected. Barriers off now means full mark-sweep only.
        if !write_barriers_enabled() {
            return false;
        }
        !matches!(
            std::env::var("PERRY_GEN_GC").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Gen-GC Phase C4b: evacuation is policy-driven by default.
/// `PERRY_GEN_GC_EVACUATE=0`, `=false`, or `=off` disables the
/// policy. `=1`, `=true`, and `=on` are accepted for compatibility
/// but mean "allow the auto-policy", not unconditional evacuation.
pub fn gen_gc_evacuate_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_GEN_GC_EVACUATE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn gc_force_evacuate_enabled() -> bool {
    gen_gc_evacuate_enabled()
        && matches!(
            std::env::var("PERRY_GC_FORCE_EVACUATE").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
}

fn gc_verify_evacuation_enabled() -> bool {
    matches!(
        std::env::var("PERRY_GC_VERIFY_EVACUATION").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

#[cfg(test)]
fn gc_collect_inner() -> u64 {
    if defer_gc_request(DeferredGcRequest::Collect(GcTriggerKind::Direct)) {
        return 0;
    }
    gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current()
}

fn gc_collect_inner_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    // Issue #745: clear the per-cycle bytes-bump flag so the next
    // gc-suppressed parse can rebaseline the trigger again. Done at
    // the top so all entry points — full GC, minor GC, manual
    // `gc()`, the malloc-count trigger path — keep the flag in sync.
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    if gen_gc_enabled() {
        return gc_collect_minor_with_trigger(trigger);
    }
    gc_collect_full_mark_sweep_with_trigger(trigger)
}

fn gc_collect_full_mark_sweep_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    gc_drain_active_budgeted_cycle();
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    GcCycleState::new_full(trigger).run_to_completion()
}

fn gc_collect_emergency_full() -> GcCollectOutcome {
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Emergency))
}

/// Last-ditch recovery for a failed heap allocation (2026-07-09 audit):
/// run one synchronous full mark-sweep and let the caller retry the
/// allocation once. Returns false (caller proceeds straight to its panic)
/// when collecting here would be unsound: re-entrant emergency, inside a
/// collection/allocation bookkeeping window, or mid-budgeted-cycle.
///
/// The workspace builds with `panic = "unwind"`, and these OOM panics
/// cross `extern "C"` frames into aborts — on a memory-limited process
/// (cgroup `memory.max`, jetsam) dying without even attempting a
/// collection wasted the one chance to shed a heap full of garbage.
///
/// The conservative stack scan is forced for the same reason the
/// alloc-point direct arm forces it: this runs at an arbitrary allocation
/// site where locals of the current call chain may not be spilled to
/// shadow slots.
pub(crate) fn gc_try_emergency_reclaim() -> bool {
    thread_local! {
        static IN_EMERGENCY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if IN_EMERGENCY.with(|c| c.get()) {
        return false;
    }
    if GC_FLAGS.with(|f| f.get()) & GC_FLAG_IN_ALLOC != 0 || gc_budgeted_cycle_active() {
        return false;
    }
    IN_EMERGENCY.with(|c| c.set(true));
    let _scan = roots::ManualGcScanGuard::force_full_scan();
    let _ = gc_collect_emergency_full();
    IN_EMERGENCY.with(|c| c.set(false));
    true
}

#[cfg(test)]
pub(super) fn test_gc_collect_emergency_full_trace_json() -> serde_json::Value {
    let outcome = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Emergency,
        steps_before: Some(GcStepSnapshot::current()),
    });
    outcome
        .trace
        .expect("test requested emergency full GC trace capture")
        .into_json(GcStepSnapshot::current())
}

thread_local! {
    /// Whether `gc_init` has registered this thread's root scanners yet. The
    /// scanner list (`MUTABLE_ROOT_SCANNERS`) is thread-local, so soundness
    /// requires every thread that can trigger a collection to register
    /// independently — not just the main thread that runs `js_gc_init()`.
    static GC_INIT_DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// When set, `ensure_gc_initialized` is a no-op. The GC unit tests take
    /// manual control of the thread's scanner registry (see
    /// `ScopedRootScannerRegistryGuard`) and must collect with exactly the
    /// roots they install — lazy auto-init would pollute that controlled set.
    static AUTO_GC_INIT_SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Suppress (or re-enable) lazy `ensure_gc_initialized` on this thread, returning
/// the previous value. Used by the GC tests' `ScopedRootScannerRegistryGuard` to
/// run collections against a hand-controlled root set.
pub(crate) fn set_auto_gc_init_suppressed(suppressed: bool) -> bool {
    AUTO_GC_INIT_SUPPRESSED.with(|c| c.replace(suppressed))
}

/// Register the runtime root scanners on the current thread if they haven't been
/// registered yet. Idempotent per thread; a no-op while auto-init is suppressed.
///
/// `js_gc_init()` runs this at the production entrypoint, but spawned worker
/// threads and the unit-test harness never call it — so without this a collection
/// on those threads runs with an empty scanner set and reclaims live objects
/// reachable only through a registered root (most importantly the realm global at
/// `GLOBAL_THIS_PTR` and the `Array`/`Object` intrinsics it holds). Called from
/// `js_get_global_this` before the global is created, so the global is born under
/// a registered scanner and survives later collections on this thread.
pub(crate) fn ensure_gc_initialized() {
    if AUTO_GC_INIT_SUPPRESSED.with(|c| c.get()) {
        return;
    }
    if !GC_INIT_DONE.with(|c| c.get()) {
        gc_init();
    }
}

pub fn gc_init() {
    // Idempotent per thread: production calls this at startup, and
    // `ensure_gc_initialized` calls it lazily on threads that don't. Latch the
    // flag before any registration so a re-entrant call can't double-register
    // the thread-local scanner list.
    if GC_INIT_DONE.with(|c| c.replace(true)) {
        return;
    }
    crate::perf_hooks::init_time_origin();
    gc_register_budgeted_mutable_root_scanner_with_source(
        scan_runtime_handle_roots_mut,
        scan_runtime_handle_roots_mut_step,
        new_runtime_handle_root_scan_state,
        MutableRootScannerSource::RuntimeHandles,
    );
    gc_register_mutable_root_scanner(crate::promise::scan_native_async_completion_roots_mut);
    gc_register_budgeted_mutable_root_scanner_with_source(
        promise_mutable_root_scanner,
        crate::promise::scan_promise_roots_mut_step,
        crate::promise::new_promise_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    gc_register_budgeted_mutable_root_scanner_with_source(
        timer_mutable_root_scanner,
        crate::timer::scan_timer_roots_mut_step,
        crate::timer::new_timer_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    // 2026-07-02 audit P0 (ported from be73b4f8d): string-keyed descriptor
    // tables (defineProperty accessors/attrs) and the proxy registry +
    // reflect-metadata store were invisible to GC — values swept/moved under
    // live references, owner keys stale after evacuation.
    gc_register_mutable_root_scanner(crate::object::descriptor_state::scan_descriptor_roots_mut);
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    // Object/string-valued `err.<prop> = v` user props live as raw bits in
    // ERROR_USER_PROPS — invisible to GC without this scanner (collectable
    // while reachable; stale addresses after a move). The address KEYS are
    // maintained by the ErrorSideTables move/finalize hooks.
    gc_register_mutable_root_scanner(
        crate::node_submodules::diagnostics_gc::scan_error_user_props_roots_mut,
    );
    gc_register_mutable_root_scanner(exception_mutable_root_scanner);
    gc_register_mutable_root_scanner(async_context_mutable_root_scanner);
    gc_register_mutable_root_scanner(async_hooks_mutable_root_scanner);
    gc_register_mutable_root_scanner(shape_cache_mutable_root_scanner);
    gc_register_mutable_root_scanner(crate::regex::scan_last_exec_groups_root_mut);
    gc_register_mutable_root_scanner(crate::object::scan_exotic_expando_roots_mut);
    gc_register_mutable_root_scanner(crate::array::scan_template_raw_roots_mut);
    gc_register_mutable_root_scanner(crate::perf_hooks::scan_perf_entries_roots_mut);
    gc_register_mutable_root_scanner(crate::v8::scan_v8_promise_hook_roots_mut);
    gc_register_mutable_root_scanner(crate::typed_feedback::scan_typed_feedback_roots_mut);
    gc_register_mutable_root_scanner(crate::typedarray_props::scan_typed_array_own_props_roots_mut);
    gc_register_mutable_root_scanner(transition_cache_mutable_root_scanner);
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_arguments_object_roots_mut);
    gc_register_budgeted_mutable_root_scanner_with_source(
        crate::object::scan_class_side_table_roots_mut,
        crate::object::scan_class_side_table_roots_mut_step,
        crate::object::new_class_side_table_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    gc_register_budgeted_mutable_root_scanner_with_source(
        crate::symbol::scan_symbol_side_table_roots_mut,
        crate::symbol::scan_symbol_side_table_roots_mut_step,
        crate::symbol::new_symbol_side_table_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    // Issue #1813: the implicit-`this` cell holds the live receiver across a
    // dynamically-dispatched method body. A moving GC triggered from inside
    // that body (e.g. @perryts/mysql Pool.acquire → handshake → nativeScramble
    // under concurrent load) must rewrite the cell, or the body's next
    // `this`-derived dispatch derefs a relocated receiver → SIGSEGV.
    gc_register_mutable_root_scanner(crate::object::scan_implicit_this_roots_mut);
    // Issue #1790 (epic #1785 class-object dispatch / design #1772): the class
    // static-inheritance side-tables CLASS_PROTOTYPE_OBJECTS and
    // CLASS_PARENT_CLOSURES hold the heap parent (`class Sub extends make(...)`
    // / `extends Context.Tag(..)()`) as a raw `usize` pointer. Root + rewrite
    // them so a parent reachable only through the table survives collection and
    // its address is fixed up after a copying-nursery / evacuation move,
    // keeping `Sub.ast` and inherited static methods resolvable.
    gc_register_mutable_root_scanner(crate::object::scan_class_inheritance_roots_mut);
    // #1934: live `child_process.spawn` ChildProcess objects are reachable only
    // from the reactor's registry (the event loop holds no JSValue root for a
    // fire-and-forget spawn). Scan + rewrite them so a GC between ticks doesn't
    // reclaim the object whose `data`/`exit` handlers are still pending.
    gc_register_mutable_root_scanner(crate::child_process::reactor::cp_reactor_scan_roots_mut);
    // #4911: a bound node:dgram socket is reachable only from the dgram
    // reactor's registry while its recv thread runs; scan + rewrite it so a GC
    // between ticks doesn't reclaim the object whose `message` handlers fire.
    #[cfg(feature = "mod-dgram")]
    gc_register_mutable_root_scanner(crate::dgram_reactor::scan_roots_mut);
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    gc_register_mutable_root_scanner(intern_table_mutable_root_scanner);
    gc_register_mutable_root_scanner(small_int_cache_mutable_root_scanner);
    gc_register_mutable_root_scanner(crate::builtins::scan_console_log_singleton_roots_mut);
    gc_register_mutable_root_scanner(crate::builtins::scan_boxed_primitive_payload_roots_mut);
    gc_register_mutable_root_scanner(crate::weakref::scan_pending_finalization_jobs_roots_mut);
    // #6182: keep the weak-holder registry's stored holder ADDRESSES current
    // across evacuation. Metadata-only (non-rooting) — it rewrites forwarded
    // addresses in rewrite phases and emits nothing during mark, so it never
    // keeps a dead holder alive. Copied-minor liveness/prune is driven by
    // `process_weak_targets_from_registry`; this covers full-cycle currency.
    gc_register_mutable_root_scanner(crate::weakref::scan_weak_holders_roots_mut);
    // Issue #841: GC roots for the per-(submodule, export) function
    // singletons + per-submodule namespace stub objects allocated by
    // `node_submodules.rs`. Without this scanner the next GC cycle
    // after first import-binding use would reclaim the singletons
    // (nothing else holds them — they live for the program's lifetime
    // via codegen `getter` calls, not via a user-visible JSValue root).
    gc_register_mutable_root_scanner(
        crate::node_submodules::scan_node_submodule_singleton_roots_mut,
    );
    // Box-capture root scanner (mutable closure captures, esp. the
    // generator state-machine's `__iter` and `__step` boxes that hold
    // the iter object + step closure across awaits).
    gc_register_mutable_root_scanner(crate::r#box::scan_box_roots_mut);
    // Iter-result scratch slot — the async-step fast path stows the
    // generator's most recent yield value here; it stays live until
    // the step driver reads it back.
    gc_register_mutable_root_scanner(crate::promise::scan_iter_result_root_mut);
    // Async-step thunk single-slot cache (build_async_step_thunks).
    gc_register_mutable_root_scanner(crate::promise::scan_async_step_thunk_cache_mut);
    // Closure singleton caches. Captured-closure cache keys mirror closure
    // capture heap words, so copied-minor must rewrite them after moving
    // captured young values or future cache hits miss on stale addresses.
    gc_register_mutable_root_scanner(crate::closure::scan_singleton_closure_roots_mut);
    gc_register_mutable_root_scanner(crate::closure::scan_closure_dynamic_props_roots_mut);
    // Generic per-handle expando properties (`blob.colors = [...]` and other
    // arbitrary own props on native HANDLE values). Keys are stable small handle
    // ids; only the stored VALUES are JS references that must be traced.
    gc_register_mutable_root_scanner(crate::object::handle_expando::scan_handle_expando_roots_mut);
    // Native-module callable export singletons and process stdio stream
    // singletons store heap pointers in TLS caches; keep them live and rewrite
    // them if a copying collection moves their backing allocations.
    gc_register_mutable_root_scanner(crate::object::scan_native_callable_export_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_class_capture_value_roots_mut);
    gc_register_mutable_root_scanner(crate::node_vm::scan_vm_roots_mut);
    gc_register_mutable_root_scanner(crate::tls::scan_tls_roots_mut);
    gc_register_mutable_root_scanner(crate::process::scan_process_finalization_roots_mut);
    gc_register_mutable_root_scanner(crate::process::scan_process_module_loader_roots_mut);
    gc_register_mutable_root_scanner(crate::os::scan_process_event_listener_roots_mut);
    // #6077: keep promises tracked for an unhandled rejection alive + address-
    // stable until reported, so the program-end report is not a stale/UAF read.
    gc_register_mutable_root_scanner(crate::promise::scan_unhandled_rejection_roots_mut);
    gc_register_mutable_root_scanner(crate::os::scan_process_stream_singleton_roots_mut);
    gc_register_mutable_root_scanner(crate::fs::scan_fs_handle_roots_mut);
    gc_register_mutable_root_scanner(crate::fs::scan_fs_stream_roots_mut);
    gc_register_mutable_root_scanner(crate::fs::scan_fs_watcher_roots_mut);
    #[cfg(feature = "full")]
    gc_register_mutable_root_scanner(crate::plugin::scan_plugin_roots_mut);
    gc_register_mutable_root_scanner(crate::geisterhand_registry::scan_geisterhand_roots_mut);
    gc_register_mutable_root_scanner(crate::ui_text_registry::scan_ui_text_registry_roots_mut);
    // perry/tui hook + state slot pools — they store raw NaN-boxed
    // value bits but the GC has no other way to know which slots hold
    // heap pointers (arrays/objects/strings stashed via setState /
    // useState / useRef). #679 follow-up: pre-fix, an Enter-press in
    // the perry-code demo stored a freshly-concat'd messages array,
    // the next allocation triggered minor GC, and the array was
    // reclaimed because nothing else held it — `messages.map(…)` on
    // the stale pointer produced an empty render.
    gc_register_budgeted_mutable_root_scanner_with_source(
        crate::tui::hooks::scan_hook_slot_roots_mut,
        crate::tui::hooks::scan_hook_slot_roots_mut_step,
        crate::tui::hooks::new_hook_slot_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    gc_register_budgeted_mutable_root_scanner_with_source(
        crate::tui::state::scan_state_slot_roots_mut,
        crate::tui::state::scan_state_slot_roots_mut_step,
        crate::tui::state::new_state_slot_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    #[cfg(feature = "ohos-napi")]
    gc_register_mutable_root_scanner(crate::arkts_callbacks::arkts_callbacks_root_scanner_mut);
}

#[no_mangle]
pub extern "C" fn js_gc_init() {
    crate::node_submodules::diagnostics_channel_init_main_thread();
    // #5093: force every class-field access back through the full guard call —
    // i.e. disable the codegen-inlined fast path — when:
    //   - typed-feedback tracing is on (the guard observes every access), or
    //   - the intact-bit verifier is on (`PERRY_VERIFY_TYPED_INTACT`): the
    //     verifier lives in the guard's fast contract, so inline hits would skip
    //     it; disabling the inline path routes every access through it, or
    //   - the explicit escape hatch `PERRY_DISABLE_CLASS_FIELD_INLINE` is set to
    //     a truthy value (perf bisection / A-B measurement). `=0`/`=false`/`=off`
    //     leave the fast path enabled.
    if crate::typed_feedback::typed_feedback_active()
        || env_flag_enabled("PERRY_VERIFY_TYPED_INTACT")
        || env_flag_enabled("PERRY_DISABLE_CLASS_FIELD_INLINE")
    {
        crate::object::disable_class_field_inline_guard();
    }
    gc_init();
}

/// #5093: parse a boolean-ish env var by value (not mere presence): true for
/// `1`/`true`/`on`/`yes` (case-insensitive), false for unset / `0`/`false`/`off`
/// / `no` / empty / anything else.
fn env_flag_enabled(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ),
        Err(_) => false,
    }
}

/// FFI: get GC stats
#[no_mangle]
pub extern "C" fn js_gc_stats(
    out_collections: *mut u64,
    out_freed: *mut u64,
    out_pause_us: *mut u64,
) {
    GC_STATS.with(|stats| {
        let stats = stats.borrow();
        unsafe {
            if !out_collections.is_null() {
                *out_collections = stats.collection_count;
            }
            if !out_freed.is_null() {
                *out_freed = stats.total_freed_bytes;
            }
            if !out_pause_us.is_null() {
                *out_pause_us = stats.last_pause_us;
            }
        }
    });
}

/// FFI: always-on pause observability (#6187, 2026-07-09 audit). Fills the
/// max pause since thread start, the max and mean over the recent-pause
/// ring (`GC_RECENT_PAUSE_WINDOW` samples), and how many samples the ring
/// currently holds. Cheap enough for a UI frame scheduler to poll per tick.
#[no_mangle]
pub extern "C" fn js_gc_pause_stats(
    out_max_us: *mut u64,
    out_recent_max_us: *mut u64,
    out_recent_avg_us: *mut u64,
    out_recent_count: *mut u64,
) {
    GC_STATS.with(|stats| {
        let stats = stats.borrow();
        let n = stats.recent_len as usize;
        let window = &stats.recent_pauses_us[..n.min(GC_RECENT_PAUSE_WINDOW)];
        let recent_max = window.iter().copied().max().unwrap_or(0);
        let recent_avg = if window.is_empty() {
            0
        } else {
            window.iter().copied().sum::<u64>() / window.len() as u64
        };
        unsafe {
            if !out_max_us.is_null() {
                *out_max_us = stats.max_pause_us;
            }
            if !out_recent_max_us.is_null() {
                *out_recent_max_us = recent_max;
            }
            if !out_recent_avg_us.is_null() {
                *out_recent_avg_us = recent_avg;
            }
            if !out_recent_count.is_null() {
                *out_recent_count = n as u64;
            }
        }
    });
}

#[cfg(test)]
mod tests;

/// Crate-wide handle on the GC test-isolation lock — see
/// `tests::support::copying_nursery_isolation_lock`. Any test OUTSIDE the gc
/// module that populates-then-asserts a process-global side table (e.g.
/// `CLOSURE_PROPS`) must hold this, or the gc test guards' global state reset
/// on a parallel test thread can wipe its entries mid-test.
#[cfg(test)]
pub(crate) use tests::support::copying_nursery_isolation_lock as global_side_table_test_lock;
