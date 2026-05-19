//! ink-API ergonomics layer (#679): `useState`, `useEffect`, `useApp`,
//! `useStdout`, `useMemo`, `useRef`.
//!
//! The existing `state()` builder (state.rs) allocates a slot per call —
//! correct for module-scope `const counter = state(0)`, wrong for hooks
//! that live inside a component body and are re-entered every render.
//! React-shape hooks bind to a call-site identity, not call count, so
//! re-rendering the same component reads back the same slot.
//!
//! We implement that with a per-render hook index that the run-loop
//! resets at the top of each frame (see `reset_hook_index`). Each
//! `useXxx` reads `NEXT_HOOK_IDX`, finds (or lazily allocates) the slot
//! at that position, and advances the counter. Slot kinds are tagged so
//! we panic on rule-of-hooks violations (calling hooks in different
//! orders across renders) instead of silently producing wrong values.
//!
//! `useState`/`useApp`/`useStdout`/`useRef` return scalar handles
//! (singleton "App"/"Stdout" handles, or NaN-boxed state-slot handles).
//! The corresponding `.exit()`/`.write()`/`.get()` methods dispatch via
//! perry-codegen NativeModSig rows that map the receiver class to the
//! runtime FFI symbol — same pattern as `state(0).get()`.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::array::{
    js_array_alloc, js_array_get_element_f64, js_array_get_length, js_array_push_f64, ArrayHeader,
};
use crate::closure::{
    js_closure_alloc, js_closure_call0, js_closure_set_capture_f64, ClosureHeader,
};
use crate::value::JSValue;

use super::state::STATE_DIRTY;

// ---------------------------------------------------------------------------
// Hook slot machinery — call-site-indexed storage that persists across
// component re-renders.
// ---------------------------------------------------------------------------

/// What kind of hook a slot holds. We tag each slot so the second
/// render's `useState` at index N can confirm the slot is still a
/// State, not a Memo / Effect / Ref. Mismatch ⇒ rule-of-hooks violation
/// (we treat as a no-op rather than panicking).
#[derive(Clone)]
enum HookSlot {
    /// Plain value cell, identical semantics to state.rs's SLOTS entry.
    State { value_bits: u64 },
    /// `useEffect(fn, deps)` — last seen deps fingerprint + optional
    /// cleanup closure pointer (reserved for cleanup-on-dep-change).
    Effect {
        last_deps_hash: u64,
        ran_once: bool,
        #[allow(dead_code)]
        cleanup: i64, // NaN-boxed POINTER to cleanup closure, or 0
    },
    /// `useMemo(fn, deps)` — cached value + last-deps fingerprint.
    Memo {
        last_deps_hash: u64,
        value_bits: u64,
        computed: bool,
    },
    /// `useRef(initial)` — mutable cell. Same storage as State but a
    /// distinct kind so a rule-of-hooks mismatch can be detected.
    Ref { value_bits: u64 },
    /// `useFocus({autoFocus, isActive})` — registers this slot as a
    /// focus candidate. Stores its assigned focus-order ID so the
    /// FocusManager's Tab cycle can route correctly across renders.
    Focus { focus_id: u32, is_active: bool },
}

static SLOTS: Mutex<Vec<HookSlot>> = Mutex::new(Vec::new());
/// Per-frame hook index, reset by the run loop before each component call.
static NEXT_HOOK_IDX: AtomicUsize = AtomicUsize::new(0);

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// GC root scanner — emits every heap-pointer-bearing slot value so
/// the tracing GC keeps `useState`/`useMemo`/`useRef` payloads alive
/// across collections.
///
/// Without this, a `setMessages(messages.concat([…]))` call (or any
/// other store-an-array-into-state pattern) leaves the new array
/// referenced only from `SLOTS[idx].value_bits`. The very next
/// allocation can trigger a minor GC that finds no other root for the
/// array and reclaims it. Subsequent renders read stale bits and
/// `messages.map(…)` produces nothing visible — the symptom that
/// surfaced in the perry-code demo (#679 follow-up).
///
/// `Effect` slots intentionally don't emit `cleanup` because it's
/// always 0 today; when cleanup-on-dep-change wiring lands the
/// scanner will need to emit it too.
pub fn scan_hook_slot_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_hook_slot_roots_mut(&mut visitor);
}

pub fn scan_hook_slot_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    for slot in s.iter_mut() {
        match slot {
            HookSlot::State { value_bits } => {
                visitor.visit_nanbox_u64_slot(value_bits);
            }
            HookSlot::Memo {
                value_bits,
                computed,
                ..
            } => {
                if *computed {
                    visitor.visit_nanbox_u64_slot(value_bits);
                }
            }
            HookSlot::Ref { value_bits } => {
                visitor.visit_nanbox_u64_slot(value_bits);
            }
            // TODO: when useEffect cleanup-on-dep-change wiring lands,
            // emit `cleanup` here too — it'll hold a NaN-boxed POINTER
            // to a Perry closure that the GC otherwise can't see.
            HookSlot::Effect { .. } | HookSlot::Focus { .. } => {}
        }
    }
}

#[cfg(test)]
pub(crate) fn test_seed_hook_slot_roots(value_bits: u64) {
    let mut slots = crate::gc::lock_gc_root_registry(&SLOTS);
    slots.clear();
    slots.push(HookSlot::State { value_bits });
    slots.push(HookSlot::Memo {
        last_deps_hash: 0,
        value_bits,
        computed: true,
    });
    slots.push(HookSlot::Ref { value_bits });
    NEXT_HOOK_IDX.store(0, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_hook_slot_roots() -> (u64, u64, u64) {
    let slots = crate::gc::lock_gc_root_registry(&SLOTS);
    let state = match slots.first() {
        Some(HookSlot::State { value_bits }) => *value_bits,
        _ => 0,
    };
    let memo = match slots.get(1) {
        Some(HookSlot::Memo { value_bits, .. }) => *value_bits,
        _ => 0,
    };
    let reference = match slots.get(2) {
        Some(HookSlot::Ref { value_bits }) => *value_bits,
        _ => 0,
    };
    (state, memo, reference)
}

#[cfg(test)]
pub(crate) fn test_with_hook_slots_locked<R>(f: impl FnOnce() -> R) -> R {
    let _slots = crate::gc::lock_gc_root_registry(&SLOTS);
    f()
}

/// Reset the hook index at the top of each render. Called by run.rs.
pub fn reset_hook_index() {
    NEXT_HOOK_IDX.store(0, Ordering::Release);
}

fn next_idx() -> usize {
    NEXT_HOOK_IDX.fetch_add(1, Ordering::AcqRel)
}

// ---------------------------------------------------------------------------
// useState — value cell tied to call-site index.
// ---------------------------------------------------------------------------

/// `useState(initial)` — returns the slot's stored value (initialised
/// to `initial` on first call). Use `useStateSetter(slotIdx, v)` to
/// write; the slot index is the same as the hook index.
///
/// To get the slot index for later .set() calls, use
/// `js_perry_tui_use_state_slot` which returns the slot index instead
/// of the value (used by the destructuring expansion when we eventually
/// ship `const [v, setV] = useState(0)`).
#[no_mangle]
pub extern "C" fn js_perry_tui_use_state(initial: f64) -> f64 {
    let idx = next_idx();
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    while s.len() <= idx {
        s.push(HookSlot::State {
            value_bits: TAG_UNDEFINED,
        });
    }
    match &mut s[idx] {
        HookSlot::State { value_bits } => {
            if *value_bits == TAG_UNDEFINED {
                *value_bits = initial.to_bits();
            }
            f64::from_bits(*value_bits)
        }
        // Wrong slot kind ⇒ rule-of-hooks violation. Re-tag and return initial.
        other => {
            *other = HookSlot::State {
                value_bits: initial.to_bits(),
            };
            initial
        }
    }
}

/// Setter for a useState slot. The slot index matches the hook index
/// observed by the matching `useState` call. Writes through to the
/// slot and flips STATE_DIRTY when the bits change. Returns undefined.
#[no_mangle]
pub extern "C" fn js_perry_tui_use_state_set(slot_idx: f64, value: f64) -> f64 {
    let idx = slot_idx as usize;
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    if let Some(HookSlot::State { value_bits }) = s.get_mut(idx) {
        let new_bits = value.to_bits();
        if *value_bits != new_bits {
            *value_bits = new_bits;
            STATE_DIRTY.store(true, Ordering::Release);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

// ---------------------------------------------------------------------------
// useState tuple form — [value, setter] for `const [v, setV] = useState(0)`.
//
// Builds a 2-element array at runtime: index 0 is the current value,
// index 1 is a real Perry closure with the slot index captured. The
// closure points at `setter_trampoline_1` which reads its first
// capture as the slot index and writes the arg through to the state
// slot, flipping STATE_DIRTY on bit-change (same semantics as
// js_perry_tui_use_state_set).
//
// This is the "React-shape useState" path the issue calls for. The
// existing `useState` (returns scalar value) is kept for callers that
// don't want destructuring — but real ink-shape code (and the issue's
// acceptance test) use this tuple form.
// ---------------------------------------------------------------------------

/// Setter trampoline — matches the Perry closure calling convention.
/// `captures[0]` (as f64) carries the slot index assigned at allocation.
#[no_mangle]
pub extern "C" fn perry_tui_state_setter_trampoline(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let slot = unsafe {
        let captures =
            (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
        *captures
    } as usize;
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    if let Some(HookSlot::State { value_bits }) = s.get_mut(slot) {
        let new_bits = value.to_bits();
        if *value_bits != new_bits {
            *value_bits = new_bits;
            STATE_DIRTY.store(true, Ordering::Release);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `useState(initial)` tuple form: returns a 2-element array
/// `[value, setter]`. Used by the destructuring rewrite at HIR-level
/// (see destructuring.rs) and directly callable as `useStateTuple`.
///
/// Returns the array as a raw pointer (i64); the dispatch table's
/// NR_PTR wraps it with POINTER_TAG. The array is GC-managed; subsequent
/// renders will allocate fresh arrays each time, with the previous one
/// reclaimed on GC. That's fine — perry's tracing GC handles it.
#[no_mangle]
pub extern "C" fn js_perry_tui_use_state_tuple(initial: f64) -> i64 {
    let idx = next_idx();
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    while s.len() <= idx {
        s.push(HookSlot::State {
            value_bits: initial.to_bits(),
        });
    }
    if !matches!(s[idx], HookSlot::State { .. }) {
        s[idx] = HookSlot::State {
            value_bits: initial.to_bits(),
        };
    }
    let current = if let HookSlot::State { value_bits } = s[idx] {
        f64::from_bits(value_bits)
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    drop(s);

    // Allocate the setter closure with 1 capture (the slot index).
    let trampoline = perry_tui_state_setter_trampoline as *const u8;
    let setter = js_closure_alloc(trampoline, 1);
    js_closure_set_capture_f64(setter, 0, idx as f64);

    // Build [value, setter_closure] array.
    let arr = js_array_alloc(2);
    let arr = js_array_push_f64(arr, current);
    let setter_boxed = JSValue::pointer(setter as *const u8);
    let arr = js_array_push_f64(arr, f64::from_bits(setter_boxed.bits()));

    arr as i64
}

/// Returns the slot index for the current `useState` hook *without*
/// consuming the hook position. Used by the destructuring rewrite to
/// build `const setV = (v) => useStateSet(__slot, v)`. (Reserved for a
/// follow-on commit — not currently emitted by the compiler.)
#[no_mangle]
pub extern "C" fn js_perry_tui_use_state_slot(initial: f64) -> f64 {
    let idx = next_idx();
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    while s.len() <= idx {
        s.push(HookSlot::State {
            value_bits: initial.to_bits(),
        });
    }
    if let HookSlot::State { .. } = s[idx] {
        // already a state slot
    } else {
        s[idx] = HookSlot::State {
            value_bits: initial.to_bits(),
        };
    }
    idx as f64
}

// ---------------------------------------------------------------------------
// useEffect — synchronous effect with dep-change detection.
// ---------------------------------------------------------------------------

/// `useEffect(fn, deps?)`:
///   - First call at this hook index ⇒ run fn.
///   - Subsequent calls ⇒ run fn iff any element of `deps` changed by
///     bit-identity (matches React's `Object.is` for primitives).
///   - `deps_array` of 0 (no deps) ⇒ run every render. ink callers
///     that pass `[]` get the run-once semantics; the deps-hash is
///     stable across renders.
///
/// `fn_closure` is an unboxed pointer to a 0-arg closure (NA_PTR). The
/// closure may return another closure (the cleanup); v1 ignores the
/// return — cleanup-on-dep-change lands in a follow-up.
/// `deps_array` is an unboxed pointer to an ArrayHeader, or 0 for no deps.
#[no_mangle]
pub extern "C" fn js_perry_tui_use_effect(fn_closure: i64, deps_array: i64) -> f64 {
    let deps_hash = hash_deps_array(deps_array);
    let idx = next_idx();
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    while s.len() <= idx {
        s.push(HookSlot::Effect {
            last_deps_hash: 0,
            ran_once: false,
            cleanup: 0,
        });
    }
    let should_run = match &s[idx] {
        HookSlot::Effect {
            last_deps_hash,
            ran_once,
            ..
        } => {
            // No deps array (deps_array == 0) ⇒ run every render.
            // Otherwise, run if first time or deps changed.
            deps_array == 0 || !*ran_once || *last_deps_hash != deps_hash
        }
        _ => true,
    };
    if should_run {
        s[idx] = HookSlot::Effect {
            last_deps_hash: deps_hash,
            ran_once: true,
            cleanup: 0,
        };
        // Drop the lock before calling user code so a re-entrant hook
        // call (effect that itself uses hooks indirectly — bad practice
        // but possible) doesn't deadlock.
        drop(s);
        if fn_closure != 0 {
            js_closure_call0(fn_closure as *const ClosureHeader);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Mix the NaN-boxed bits of every element of a deps array into a
/// 64-bit hash. FNV-1a — fast, no collisions for the small N (usually
/// 1–4) typical of dep arrays. Returns 0 for null/empty arrays.
fn hash_deps_array(deps_array: i64) -> u64 {
    if deps_array == 0 {
        return 0;
    }
    let arr = deps_array as *const ArrayHeader;
    if arr.is_null() {
        return 0;
    }
    let len = js_array_get_length(deps_array);
    if len <= 0 {
        // Empty deps `[]` → use a fixed non-zero hash so the
        // first-call-only semantics work (last_hash stays at this
        // value forever; comparisons match).
        return 0x9e37_79b9_7f4a_7c15;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for i in 0..len {
        let v = js_array_get_element_f64(deps_array, i);
        let bits = v.to_bits();
        // FNV-1a over 8 bytes.
        for b in bits.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    // Guard against the rare 0 collision — 0 is our "no deps" sentinel.
    if h == 0 {
        1
    } else {
        h
    }
}

// ---------------------------------------------------------------------------
// useMemo — cached compute keyed by deps.
// ---------------------------------------------------------------------------

/// `useMemo(fn, deps)` — runs fn() on first call or when deps change,
/// caches the result, returns the cached value otherwise. Deps are
/// hashed the same way as useEffect (FNV-1a over per-element bits).
#[no_mangle]
pub extern "C" fn js_perry_tui_use_memo(fn_closure: i64, deps_array: i64) -> f64 {
    let deps_hash = hash_deps_array(deps_array);
    let idx = next_idx();
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    while s.len() <= idx {
        s.push(HookSlot::Memo {
            last_deps_hash: 0,
            value_bits: TAG_UNDEFINED,
            computed: false,
        });
    }
    let should_compute = match &s[idx] {
        HookSlot::Memo {
            last_deps_hash,
            computed,
            ..
        } => !*computed || *last_deps_hash != deps_hash,
        _ => true,
    };
    if should_compute {
        drop(s);
        let value = if fn_closure != 0 {
            js_closure_call0(fn_closure as *const ClosureHeader)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
        let mut s2 = crate::gc::lock_gc_root_registry(&SLOTS);
        // Slot index hasn't shifted because we only allocate, never remove.
        s2[idx] = HookSlot::Memo {
            last_deps_hash: deps_hash,
            value_bits: value.to_bits(),
            computed: true,
        };
        return value;
    }
    if let HookSlot::Memo { value_bits, .. } = &s[idx] {
        f64::from_bits(*value_bits)
    } else {
        f64::from_bits(TAG_UNDEFINED)
    }
}

// ---------------------------------------------------------------------------
// useRef — mutable cell with stable identity across renders.
// ---------------------------------------------------------------------------

/// `useRef(initial)` — returns a stable handle whose `.get()` /
/// `.set(v)` round-trip to the slot. Different from useState: writes
/// do NOT flip STATE_DIRTY, so .set() doesn't trigger a re-render
/// (matches React).
///
/// The handle is the slot index + 1 (so the encoding is never 0,
/// which the dispatch layer treats as a null pointer). The dispatch
/// table NR_PTR-wraps the i64 with POINTER_TAG; receiver-method
/// dispatch unboxes it back to an i64. We subtract 1 in `ref_get` /
/// `ref_set` to recover the slot index.
#[no_mangle]
pub extern "C" fn js_perry_tui_use_ref(initial: f64) -> i64 {
    let idx = next_idx();
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    while s.len() <= idx {
        s.push(HookSlot::Ref {
            value_bits: initial.to_bits(),
        });
    }
    if !matches!(s[idx], HookSlot::Ref { .. }) {
        s[idx] = HookSlot::Ref {
            value_bits: initial.to_bits(),
        };
    }
    (idx as i64) + 1
}

/// `ref.get()` — read the slot's stored value. `handle` is the
/// NaN-unboxed i64 receiver (slot index + 1).
#[no_mangle]
pub extern "C" fn js_perry_tui_ref_get(handle: i64) -> f64 {
    if handle <= 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let idx = (handle - 1) as usize;
    let s = crate::gc::lock_gc_root_registry(&SLOTS);
    match s.get(idx) {
        Some(HookSlot::Ref { value_bits }) => f64::from_bits(*value_bits),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}

/// `ref.set(v)` — write the slot. Does NOT flip STATE_DIRTY.
#[no_mangle]
pub extern "C" fn js_perry_tui_ref_set(handle: i64, value: f64) -> f64 {
    if handle <= 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let idx = (handle - 1) as usize;
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    if let Some(HookSlot::Ref { value_bits }) = s.get_mut(idx) {
        *value_bits = value.to_bits();
    }
    f64::from_bits(TAG_UNDEFINED)
}

// ---------------------------------------------------------------------------
// useApp — singleton handle with .exit() / .waitUntilExit() methods.
// ---------------------------------------------------------------------------

/// Singleton App handle value (slot 0 of an "app singleton" namespace).
/// Returning the same handle on every call keeps reference semantics
/// stable across renders — ink's useApp() also returns a stable object.
const APP_HANDLE: i64 = 1;

/// `useApp()` — returns an App handle whose `.exit()` and
/// `.waitUntilExit()` methods dispatch through perry-codegen's
/// class_filter: Some("App") rows.
#[no_mangle]
pub extern "C" fn js_perry_tui_use_app() -> i64 {
    APP_HANDLE
}

/// `app.exit()` — flips the run-loop's EXIT_FLAG. Receiver argument is
/// the App handle (ignored — there's only one).
#[no_mangle]
pub extern "C" fn js_perry_tui_app_exit(_handle: i64) -> f64 {
    super::input::EXIT_FLAG.store(true, Ordering::Release);
    f64::from_bits(TAG_UNDEFINED)
}

/// `app.waitUntilExit()` — busy-waits (with a small sleep) until
/// EXIT_FLAG is set. This is a synchronous block; the caller can
/// `await` the returned undefined safely (Perry's promise machinery
/// treats non-promises as already-resolved). For perry/tui v1 this is
/// good enough — the run loop itself blocks on input, so users
/// typically don't need waitUntilExit() outside an effect.
#[no_mangle]
pub extern "C" fn js_perry_tui_app_wait_until_exit(_handle: i64) -> f64 {
    use std::time::Duration;
    while !super::input::EXIT_FLAG.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(50));
    }
    f64::from_bits(TAG_UNDEFINED)
}

// ---------------------------------------------------------------------------
// useStdout — singleton handle with .write() / .columns() / .rows() methods.
// ---------------------------------------------------------------------------

const STDOUT_HANDLE: i64 = 2;

/// `useStdout()` — returns a Stdout handle whose `.write(str)`,
/// `.columns()`, `.rows()` methods dispatch through class_filter:
/// Some("Stdout") rows.
#[no_mangle]
pub extern "C" fn js_perry_tui_use_stdout() -> i64 {
    STDOUT_HANDLE
}

/// `stdout.write(s)` — write the string to stdout raw. Used as the
/// ink escape-hatch for emitting content that bypasses the TUI's
/// cell-grid diff (e.g. printing logs interleaved with the rendered UI).
#[no_mangle]
pub extern "C" fn js_perry_tui_stdout_write(
    _handle: i64,
    s_ptr: *const crate::string::StringHeader,
) -> f64 {
    use std::io::Write;
    if !s_ptr.is_null() {
        let s = unsafe {
            let len = (*s_ptr).byte_len as usize;
            let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
            std::slice::from_raw_parts(data, len)
        };
        let stdout = std::io::stdout();
        let mut h = stdout.lock();
        let _ = h.write_all(s);
        let _ = h.flush();
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `stdout.columns()` — current terminal column count. Used by ink
/// programs that want to size content to terminal width. Falls back to
/// 80 when stdout isn't a TTY.
#[no_mangle]
pub extern "C" fn js_perry_tui_stdout_columns(_handle: i64) -> f64 {
    let (w, _h) = term_size();
    w as f64
}

/// `stdout.rows()` — current terminal row count. Falls back to 24.
#[no_mangle]
pub extern "C" fn js_perry_tui_stdout_rows(_handle: i64) -> f64 {
    let (_w, h) = term_size();
    h as f64
}

fn term_size() -> (u16, u16) {
    #[cfg(unix)]
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            return (ws.ws_col, ws.ws_row);
        }
    }
    (80, 24)
}

// ---------------------------------------------------------------------------
// useFocus + useFocusManager — Tab-cyclable focus ring (#679 Phase 3).
//
// Each `useFocus(autoFocus, isActive)` call at a unique hook position
// is assigned a monotonic focus_id at first observation. The
// FocusManager tracks the currently focused id; Tab/Shift-Tab cycle
// through the registered active ids in registration order. The hook
// returns 1.0 when its id == FOCUS_CURRENT and 0.0 otherwise.
//
// The Tab/Shift-Tab cycle hooks live in input.rs's drain_input —
// `\x09` (TAB) calls focus_next, ESC-[Z (Shift-Tab) calls focus_prev.
// ---------------------------------------------------------------------------

static FOCUS_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
/// Currently focused id. 0 means "no focus yet"; the first useFocus
/// call gets id 1 (we reserve 0 for "unset" so the autoFocus path
/// has a meaningful "none assigned yet" signal).
static FOCUS_CURRENT: AtomicU64 = AtomicU64::new(0);

/// `useFocus(autoFocus, isActive)` — returns 1.0 when this widget is
/// the currently focused one, else 0.0. autoFocus=1 means take focus
/// on the first render. isActive=0 removes this widget from the Tab
/// cycle (matches ink's `isActive: false`).
#[no_mangle]
pub extern "C" fn js_perry_tui_use_focus(auto_focus: f64, is_active: f64) -> f64 {
    let idx = next_idx();
    let auto = auto_focus != 0.0;
    let active = is_active != 0.0;
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    while s.len() <= idx {
        let new_id = FOCUS_ID_COUNTER.fetch_add(1, Ordering::AcqRel) + 1;
        let take_focus = auto && FOCUS_CURRENT.load(Ordering::Acquire) == 0;
        s.push(HookSlot::Focus {
            focus_id: new_id as u32,
            is_active: active,
        });
        if take_focus {
            FOCUS_CURRENT.store(new_id, Ordering::Release);
        }
    }
    if let HookSlot::Focus {
        focus_id,
        is_active,
    } = &mut s[idx]
    {
        *is_active = active;
        let cur = FOCUS_CURRENT.load(Ordering::Acquire) as u32;
        if cur == *focus_id {
            return 1.0;
        }
    }
    0.0
}

/// `focusNext()` — advance focus to the next registered active widget.
/// Wraps around at the end of the ring. Called by the Tab key handler
/// in input.rs and by user code via useFocusManager().focusNext().
#[no_mangle]
pub extern "C" fn js_perry_tui_focus_next() -> f64 {
    focus_step(true);
    f64::from_bits(TAG_UNDEFINED)
}

/// `focusPrevious()` — opposite of focusNext.
#[no_mangle]
pub extern "C" fn js_perry_tui_focus_previous() -> f64 {
    focus_step(false);
    f64::from_bits(TAG_UNDEFINED)
}

/// `focus(id)` — focus a specific id by number. Mostly useful from
/// useFocusManager().focus(...) for tests; ink users typically use
/// focusNext/Previous.
#[no_mangle]
pub extern "C" fn js_perry_tui_focus(id: f64) -> f64 {
    let target = id as u64;
    if target > 0 {
        FOCUS_CURRENT.store(target, Ordering::Release);
        STATE_DIRTY.store(true, Ordering::Release);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn focus_step(forward: bool) {
    let s = crate::gc::lock_gc_root_registry(&SLOTS);
    let active_ids: Vec<u32> = s
        .iter()
        .filter_map(|slot| {
            if let HookSlot::Focus {
                focus_id,
                is_active,
            } = slot
            {
                if *is_active {
                    Some(*focus_id)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();
    if active_ids.is_empty() {
        return;
    }
    let cur = FOCUS_CURRENT.load(Ordering::Acquire) as u32;
    let cur_idx = active_ids.iter().position(|x| *x == cur);
    let next = match cur_idx {
        Some(i) => {
            let n = active_ids.len();
            let new_i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            active_ids[new_i]
        }
        None => *active_ids.first().unwrap(),
    };
    FOCUS_CURRENT.store(next as u64, Ordering::Release);
    STATE_DIRTY.store(true, Ordering::Release);
}

/// `useFocusManager()` — singleton handle with focus-control methods.
/// Methods dispatch via class_filter Some("FocusManager") rows.
const FOCUS_MANAGER_HANDLE: i64 = 3;

#[no_mangle]
pub extern "C" fn js_perry_tui_use_focus_manager() -> i64 {
    FOCUS_MANAGER_HANDLE
}

#[no_mangle]
pub extern "C" fn js_perry_tui_focus_manager_focus_next(_handle: i64) -> f64 {
    js_perry_tui_focus_next()
}

#[no_mangle]
pub extern "C" fn js_perry_tui_focus_manager_focus_previous(_handle: i64) -> f64 {
    js_perry_tui_focus_previous()
}

#[no_mangle]
pub extern "C" fn js_perry_tui_focus_manager_focus(_handle: i64, id: f64) -> f64 {
    js_perry_tui_focus(id)
}

// ---------------------------------------------------------------------------
// Top-level exit() and waitUntilExit() — convenience functions that
// don't require obtaining the App handle first. These match ink's
// imperative escape hatches (`process.exit()` style).
// ---------------------------------------------------------------------------

/// Top-level wait-until-exit, callable without useApp(). Identical to
/// `app.waitUntilExit()` minus the receiver arg.
#[no_mangle]
pub extern "C" fn js_perry_tui_wait_until_exit() -> f64 {
    js_perry_tui_app_wait_until_exit(APP_HANDLE)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Cargo runs tests in parallel by default. The hook slot pool +
    /// hook index counter are process-wide globals, so two tests racing
    /// see each other's writes. This mutex serialises tests within
    /// this module — `parking_lot` would be nicer for poisoning
    /// resistance but std::sync::Mutex is enough for tests.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    /// Acquire the test lock and reset all shared state. Returns the
    /// guard — drop at end of test.
    fn reset() -> std::sync::MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        crate::gc::lock_gc_root_registry(&SLOTS).clear();
        NEXT_HOOK_IDX.store(0, Ordering::Release);
        STATE_DIRTY.store(false, Ordering::Release);
        FOCUS_ID_COUNTER.store(0, Ordering::Release);
        FOCUS_CURRENT.store(0, Ordering::Release);
        super::super::input::EXIT_FLAG.store(false, Ordering::Release);
        g
    }

    #[test]
    fn use_state_initial_value_first_call() {
        let _g = reset();
        let v = js_perry_tui_use_state(42.0);
        assert_eq!(v.to_bits(), 42.0_f64.to_bits());
    }

    #[test]
    fn use_state_persists_across_renders() {
        let _g = reset();
        // First render
        let v1 = js_perry_tui_use_state(0.0);
        assert_eq!(v1, 0.0);
        // Write to slot 0
        js_perry_tui_use_state_set(0.0, 7.0);
        // Second render — hook index resets, useState reads back 7
        reset_hook_index();
        let v2 = js_perry_tui_use_state(0.0);
        assert_eq!(v2, 7.0);
    }

    #[test]
    fn use_state_set_flips_dirty() {
        let _g = reset();
        let _ = js_perry_tui_use_state(0.0);
        assert!(!STATE_DIRTY.load(Ordering::Acquire));
        js_perry_tui_use_state_set(0.0, 1.0);
        assert!(STATE_DIRTY.load(Ordering::Acquire));
    }

    #[test]
    fn use_state_two_slots_different_values() {
        let _g = reset();
        let a = js_perry_tui_use_state(1.0);
        let b = js_perry_tui_use_state(2.0);
        assert_eq!(a, 1.0);
        assert_eq!(b, 2.0);
        js_perry_tui_use_state_set(0.0, 10.0);
        js_perry_tui_use_state_set(1.0, 20.0);
        reset_hook_index();
        let a2 = js_perry_tui_use_state(1.0);
        let b2 = js_perry_tui_use_state(2.0);
        assert_eq!(a2, 10.0);
        assert_eq!(b2, 20.0);
    }

    #[test]
    fn use_ref_does_not_flip_dirty() {
        let _g = reset();
        let h = js_perry_tui_use_ref(0.0);
        assert!(!STATE_DIRTY.load(Ordering::Acquire));
        js_perry_tui_ref_set(h, 99.0);
        // Ref writes must not trigger re-render.
        assert!(!STATE_DIRTY.load(Ordering::Acquire));
        assert_eq!(js_perry_tui_ref_get(h), 99.0);
    }

    #[test]
    fn use_ref_stable_across_renders() {
        let _g = reset();
        let h1 = js_perry_tui_use_ref(0.0);
        js_perry_tui_ref_set(h1, 5.0);
        reset_hook_index();
        let h2 = js_perry_tui_use_ref(0.0);
        assert_eq!(h1, h2);
        assert_eq!(js_perry_tui_ref_get(h2), 5.0);
    }

    #[test]
    fn use_app_returns_stable_handle() {
        let _g = reset();
        let h1 = js_perry_tui_use_app();
        let h2 = js_perry_tui_use_app();
        assert_eq!(h1, h2);
    }

    #[test]
    fn app_exit_flips_flag() {
        let _g = reset();
        let h = js_perry_tui_use_app();
        assert!(!super::super::input::EXIT_FLAG.load(Ordering::Acquire));
        js_perry_tui_app_exit(h);
        assert!(super::super::input::EXIT_FLAG.load(Ordering::Acquire));
    }

    #[test]
    fn use_stdout_columns_rows_nonzero() {
        let _g = reset();
        let h = js_perry_tui_use_stdout();
        let cols = js_perry_tui_stdout_columns(h);
        let rows = js_perry_tui_stdout_rows(h);
        assert!(cols >= 1.0);
        assert!(rows >= 1.0);
    }

    #[test]
    fn use_focus_auto_focus_first_widget_gets_focus() {
        let _g = reset();
        // First widget has autoFocus=1; it should be focused.
        let f0 = js_perry_tui_use_focus(1.0, 1.0);
        // Second widget has autoFocus=0; not focused.
        let f1 = js_perry_tui_use_focus(0.0, 1.0);
        assert_eq!(f0, 1.0);
        assert_eq!(f1, 0.0);
    }

    #[test]
    fn focus_next_cycles_through_active_widgets() {
        let _g = reset();
        // 3 active focus slots, first auto-focused.
        let _ = js_perry_tui_use_focus(1.0, 1.0);
        let _ = js_perry_tui_use_focus(0.0, 1.0);
        let _ = js_perry_tui_use_focus(0.0, 1.0);
        // Cycle next, re-render, check focus moved to second slot.
        js_perry_tui_focus_next();
        reset_hook_index();
        let f0 = js_perry_tui_use_focus(1.0, 1.0);
        let f1 = js_perry_tui_use_focus(0.0, 1.0);
        let f2 = js_perry_tui_use_focus(0.0, 1.0);
        assert_eq!(f0, 0.0);
        assert_eq!(f1, 1.0);
        assert_eq!(f2, 0.0);
        // Wraps around: prev twice from idx 1 → wraps to idx 2.
        js_perry_tui_focus_previous();
        js_perry_tui_focus_previous();
        reset_hook_index();
        let f0b = js_perry_tui_use_focus(1.0, 1.0);
        let f1b = js_perry_tui_use_focus(0.0, 1.0);
        let f2b = js_perry_tui_use_focus(0.0, 1.0);
        assert_eq!(f0b, 0.0);
        assert_eq!(f1b, 0.0);
        assert_eq!(f2b, 1.0);
    }

    #[test]
    fn focus_next_skips_inactive_widgets() {
        let _g = reset();
        // Slot 0: active + auto-focused.
        let _ = js_perry_tui_use_focus(1.0, 1.0);
        // Slot 1: inactive (isActive=0) — must be skipped.
        let _ = js_perry_tui_use_focus(0.0, 0.0);
        // Slot 2: active.
        let _ = js_perry_tui_use_focus(0.0, 1.0);
        js_perry_tui_focus_next();
        reset_hook_index();
        let f0 = js_perry_tui_use_focus(1.0, 1.0);
        let f1 = js_perry_tui_use_focus(0.0, 0.0);
        let f2 = js_perry_tui_use_focus(0.0, 1.0);
        assert_eq!(f0, 0.0);
        assert_eq!(f1, 0.0);
        assert_eq!(f2, 1.0);
    }

    #[test]
    fn focus_flips_dirty_for_re_render() {
        let _g = reset();
        let _ = js_perry_tui_use_focus(1.0, 1.0);
        let _ = js_perry_tui_use_focus(0.0, 1.0);
        STATE_DIRTY.store(false, Ordering::Release);
        js_perry_tui_focus_next();
        assert!(STATE_DIRTY.load(Ordering::Acquire));
    }
}
