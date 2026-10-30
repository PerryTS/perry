//! Precise shadow-stack roots.
//!
//! # Layout
//!
//! One thread-local `Vec<ShadowEntry>` holds every frame back-to-back. A frame
//! is `SHADOW_STACK_HEADER_SLOTS` header entries followed by its slots:
//!
//! ```text
//!   [ header: value = caller frame_top, meta = slot_count ][ slot 0 ][ slot 1 ] ...
//!   ^ frame_handle (base)                                  ^ frame_top
//! ```
//!
//! Before this rewrite the three per-slot words lived in three parallel `Vec`s
//! (`stack: Vec<u64>`, `slot_ptrs: Vec<usize>`, `active: Vec<bool>`) and the
//! header took two `u64` words. That cost every frame push five separate
//! capacity checks and three `resize` calls — two of which lowered to a
//! `memset` **call** for the 2–4-slot frames codegen actually emits — and cost
//! every slot store three independent bounds checks against three `Vec`
//! headers. Interleaving the three words into one 16-byte entry makes a push
//! one capacity check plus a handful of `stp`s, and a slot store one bounds
//! check against one length.

use std::cell::UnsafeCell;
use std::sync::atomic::Ordering;

pub(crate) use super::shadow_layout::{ShadowEntry, SLOT_ACTIVE, SLOT_PTR_MASK};
use super::shadow_layout::{
    ShadowStackState, SHADOW_STACK_GROW_RESERVE, SHADOW_STACK_HEADER_SLOTS,
};

impl ShadowStackState {
    /// The live entries as a slice. Empty (and never dereferencing a null
    /// `ptr`) before the first growth.
    #[inline(always)]
    pub(crate) fn slots(&self) -> &[ShadowEntry] {
        if self.ptr.is_null() {
            return &[];
        }
        // SAFETY: once `ptr` is non-null it covers `cap >= len` entries, and
        // every write path sets `len` only after initializing the range.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// The live entries as a mutable slice.
    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) fn slots_mut(&mut self) -> &mut [ShadowEntry] {
        if self.ptr.is_null() {
            return &mut [];
        }
        // SAFETY: as [`ShadowStackState::slots`].
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Drop every frame, keeping the buffer. Test/reset helper.
    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) fn clear_slots_for_reset(&mut self) {
        self.len = 0;
    }

    /// # Safety
    /// `new_len <= cap`, and entries in `0..new_len` are initialized.
    #[inline(always)]
    pub(crate) unsafe fn set_slots_len(&mut self, new_len: usize) {
        debug_assert!(new_len <= self.cap);
        self.len = new_len;
    }
}

// [`SHADOW`]'s storage mechanism is cfg-split by platform. Both arms are
// `const`-initialized and drop-free, so neither pays a lazy-init or
// destructor-registration check; the buffer is reserved lazily on the first
// push instead of eagerly at thread start, and released by
// [`ShadowBufferGuard`], on either arm.
//
// # Why `perry_thread_local!` only on Apple aarch64
//
// Until #10619, `SHADOW` was unconditionally a raw `thread_local!`: every
// `try`/`catch` paid this exact resolution once per entry, through the
// shadow-frame savepoint a `try` used to capture. Profiling a `try`/`catch`
// loop showed it as a genuine leaf `_tlv_get_addr` call, ~6% of total
// instructions retired per non-throwing `try` entry. (Since ehfree slice 1 a
// `try` captures nothing; the hot shadow paths are frame push/pop and slot
// stores.)
//
// But that fast path is itself Darwin-aarch64-specific: `tls_hot.rs`'s own
// module docs say the published-cache shortcut only exists there, and
// everywhere else "resolving a thread-local is already a fixed offset and
// the extra cache indirection has no demonstrated benefit." #10619 shipped
// the swap unconditionally anyway, and CI's `cargo-test` job — Linux, debug
// profile — hit a SIGSEGV that a plain `thread_local!` never produced
// (`cargo-test` run 35374727647, job 105594641738). Local reproduction (macOS
// on both the Darwin path and with it forced off, and a qemu-emulated Linux
// x86_64 VM) never faulted, so causation was confirmed the direct way
// instead: `cargo-test` on CI itself, gating `SHADOW` back to this cfg split,
// came back green (run 35433215970, job 105871415920) at the same commit
// that was red with the swap unconditional — so the PR *is* what caused it.
// The internal mechanism is still not understood: this fixes it by removing
// the change from every platform where it had no benefit anyway, not by
// finding the fault inside `tls_hot.rs`'s resolution path. See #10709 for
// the open half of the investigation (`fill()`'s `temp_roots`-last ordering
// guards against a half-filled cache being *used* re-entrantly, not against
// `fill()` being *called* re-entrantly, which remains a live suspect).
//
// Since the −8% `try`-entry win was only ever measured on Darwin
// (`perry_thread_local!`'s whole premise doesn't apply anywhere the direct
// TLS access is already a fixed offset — see `tls_hot.rs`), routing through
// it off that platform was strictly more work for no measured benefit even
// before the SIGSEGV: one more thread-local resolution (`HOT`) plus a
// slot-array indirection, replacing the single direct TLS access a raw
// `thread_local!` already was. So the fast path now ships only where it was
// measured, and every other target keeps the original raw form — which
// also means this file's `thread_local!` count goes back to what it was
// before #10619 (see `scripts/thread_local_cold_allowlist.json`).
#[cfg(all(
    target_vendor = "apple",
    target_arch = "aarch64",
    target_pointer_width = "64"
))]
crate::perry_thread_local! {
    /// See the cfg-split rationale above this declaration.
    pub(crate) static SHADOW: UnsafeCell<ShadowStackState> = const {
        UnsafeCell::new(ShadowStackState {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
            frame_top: usize::MAX,
        })
    };
}

#[cfg(not(any(
    all(
        target_vendor = "apple",
        target_arch = "aarch64",
        target_pointer_width = "64"
    ),
    all(target_family = "wasm", not(target_feature = "atomics"))
)))]
thread_local! {
    /// See the cfg-split rationale above this declaration's sibling arm.
    pub(crate) static SHADOW: UnsafeCell<ShadowStackState> = const {
        UnsafeCell::new(ShadowStackState {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
            frame_top: usize::MAX,
        })
    };
}

/// Single-threaded wasm (no `atomics`): the program has exactly one thread,
/// so the state is one exported static rather than a thread-local. Generated
/// code addresses it by symbol and pushes and pops frames inline
/// (`perry-codegen`'s `wasm32::shadow_frame`), taking `js_shadow_frame_enter` /
/// `js_shadow_frame_pop` only to grow the buffer or to report a bad handle.
/// [`SHADOW`] keeps the `thread_local!` access API, so every runtime path below
/// is the same code on every target.
#[cfg(all(target_family = "wasm", not(target_feature = "atomics")))]
#[repr(transparent)]
pub struct ShadowStateCell(UnsafeCell<ShadowStackState>);

// SAFETY: a wasm module without `atomics` cannot run a second thread.
#[cfg(all(target_family = "wasm", not(target_feature = "atomics")))]
unsafe impl Sync for ShadowStateCell {}

/// The shadow-stack state generated code reads and writes inline on
/// single-threaded wasm. Part of the codegen contract: its layout is
/// [`ShadowStackState`]'s.
#[cfg(all(target_family = "wasm", not(target_feature = "atomics")))]
#[no_mangle]
pub static PERRY_SHADOW_STATE: ShadowStateCell =
    ShadowStateCell(UnsafeCell::new(ShadowStackState {
        ptr: std::ptr::null_mut(),
        len: 0,
        cap: 0,
        frame_top: usize::MAX,
    }));

/// [`SHADOW`] on single-threaded wasm: `thread_local!`'s access API over
/// [`PERRY_SHADOW_STATE`].
#[cfg(all(target_family = "wasm", not(target_feature = "atomics")))]
pub(crate) struct ShadowKey;

#[cfg(all(target_family = "wasm", not(target_feature = "atomics")))]
impl ShadowKey {
    #[inline(always)]
    pub(crate) fn with<R>(&'static self, f: impl FnOnce(&UnsafeCell<ShadowStackState>) -> R) -> R {
        f(&PERRY_SHADOW_STATE.0)
    }

    /// Never fails: the static outlives every caller.
    #[inline(always)]
    pub(crate) fn try_with<R>(
        &'static self,
        f: impl FnOnce(&UnsafeCell<ShadowStackState>) -> R,
    ) -> Result<R, std::convert::Infallible> {
        Ok(self.with(f))
    }
}

#[cfg(all(target_family = "wasm", not(target_feature = "atomics")))]
pub(crate) static SHADOW: ShadowKey = ShadowKey;

/// Frees the shadow buffer at thread exit.
///
/// [`ShadowStackState`] is deliberately drop-free (see its docs), so the
/// allocation is owned by this separate thread-local instead. It is armed from
/// the cold [`grow_for`] path, so a thread that never pushes a frame never
/// registers a destructor.
///
/// The destructor resets the state to the *empty* sentinel rather than leaving
/// a dangling `ptr`, so any shadow-stack op that runs after this point sees
/// "no frame, no buffer" and no-ops. That is strictly safer than the previous
/// behaviour: a `Vec` inside the TLS made the whole slot `DESTROYED`, and the
/// next access aborted through `std`'s `panic_access_error`.
struct ShadowBufferGuard;

impl Drop for ShadowBufferGuard {
    fn drop(&mut self) {
        // `SHADOW` is drop-free, so it is never itself destroyed and this
        // access cannot fail regardless of destructor order.
        let _ = SHADOW.try_with(|cell| unsafe {
            let s = &mut *cell.get();
            let (ptr, cap) = (s.ptr, s.cap);
            s.ptr = std::ptr::null_mut();
            s.len = 0;
            s.cap = 0;
            s.frame_top = usize::MAX;
            if !ptr.is_null() && cap != 0 {
                drop(Vec::from_raw_parts(ptr, 0, cap));
            }
        });
    }
}

thread_local! {
    static SHADOW_BUFFER_GUARD: ShadowBufferGuard = const { ShadowBufferGuard };
}

/// Reserve room for `need` more entries. Outlined and `#[cold]` so the push
/// fast path stays a capacity compare and a not-taken branch.
///
/// Growth reallocates the buffer, so `ShadowStackState::ptr` is **not** stable
/// across a push. Generated code therefore re-loads `ptr` from the state at
/// every inline slot store rather than caching a frame base address; only the
/// address of the state struct itself (a thread-local, fixed for the thread's
/// lifetime) is cached per activation.
#[cold]
#[inline(never)]
fn grow_for(s: &mut ShadowStackState, need: usize) {
    // Arm the thread-exit free the first time this thread allocates.
    let _ = SHADOW_BUFFER_GUARD.try_with(|_| ());
    let want = s
        .len
        .saturating_add(need.max(SHADOW_STACK_GROW_RESERVE))
        .max(s.cap.saturating_mul(2));
    // Round-trip through `Vec` so the allocation is made and freed with the
    // same allocator and layout that `ShadowBufferGuard` releases.
    let mut v: Vec<ShadowEntry> = if s.ptr.is_null() {
        Vec::new()
    } else {
        // SAFETY: `ptr`/`cap` came from a `Vec<ShadowEntry>` built here. Length
        // is passed as 0 because `ShadowEntry: Copy` has no drop glue, and the
        // live prefix is copied by `reserve` from the raw allocation anyway —
        // so re-declare the real length to keep the data.
        unsafe { Vec::from_raw_parts(s.ptr, s.len, s.cap) }
    };
    v.reserve(want.saturating_sub(v.len()));
    s.ptr = v.as_mut_ptr();
    s.cap = v.capacity();
    s.len = v.len();
    std::mem::forget(v);
}

/// Slots a push always zeroes, whether or not the frame declares that many.
///
/// A *constant*-size clear is the point, and it took three attempts to get one
/// that survived the optimizer. Every length-dependent form — a `match` on `n`
/// with a spelled-out arm per size, a bounded loop, `write_bytes` with a
/// runtime `n` — is re-formed by LLVM into a compare chain that computes a byte
/// count and **calls `memset`**, which is the per-activation call this rewrite
/// exists to delete. Even the constant store below was tail-merged with the
/// large-frame `write_bytes` into a single `csel`-the-length-then-`bl memset`
/// until the large path moved behind `#[inline(never)]`. All three shapes were
/// read out of the linked archive's disassembly, not assumed.
///
/// Four covers the frame sizes codegen actually emits and still lowers to a
/// pair of `stp q0, q0`.
const SHADOW_FRAME_ZERO_MIN: usize = 4;

/// Zero the `n` freshly-claimed slot entries of a new frame.
///
/// # Safety
/// `p` must point at `max(n, SHADOW_FRAME_ZERO_MIN)` writable, correctly
/// aligned `ShadowEntry`s. `js_shadow_frame_push` guarantees this by sizing
/// its capacity check with [`frame_zero_span`]; the entries past `n` are
/// inside the buffer's spare capacity and are re-zeroed by whichever push
/// claims them next.
#[inline(always)]
unsafe fn clear_slots(p: *mut ShadowEntry, n: usize) {
    if n <= SHADOW_FRAME_ZERO_MIN {
        std::ptr::write(
            p.cast::<[ShadowEntry; SHADOW_FRAME_ZERO_MIN]>(),
            [ShadowEntry::EMPTY; SHADOW_FRAME_ZERO_MIN],
        );
    } else {
        clear_large_frame_slots(p, n);
    }
}

/// Out-of-line so LLVM cannot tail-merge this variable-length `memset` with the
/// constant-length store in [`clear_slots`]. Frames this wide are rare enough
/// that the call is irrelevant to them and fatal to everything else.
///
/// # Safety
/// `p` must point at `n` writable, correctly-aligned `ShadowEntry`s.
#[cold]
#[inline(never)]
unsafe fn clear_large_frame_slots(p: *mut ShadowEntry, n: usize) {
    std::ptr::write_bytes(p, 0, n);
}

/// Entries a push must have room for: the header, the declared slots, and the
/// over-zeroed tail [`clear_slots`] writes for small frames.
#[inline(always)]
fn frame_zero_span(slot_count: usize) -> usize {
    SHADOW_STACK_HEADER_SLOTS
        + if slot_count < SHADOW_FRAME_ZERO_MIN {
            SHADOW_FRAME_ZERO_MIN
        } else {
            slot_count
        }
}

/// Encode a bound compiled-local address into a live [`ShadowEntry::bound`].
///
/// Bit 0 carries the liveness flag, so an address whose bit 0 is set cannot be
/// recorded. Rather than truncate it — which would hand the collector a
/// *different* address to write a forwarded pointer into — the binding is
/// dropped and the entry stays active-but-unbound: the mirrored value is still
/// marked and still rewritten, only the write-through to the compiled local is
/// lost. Compiled `i64`/`double` local slots are always 8-byte aligned, so the
/// fallback is unreachable in practice; it exists so a mis-emitted callsite
/// degrades instead of corrupting memory. Branchless (`tst`/`csel`), so the
/// aligned case pays nothing.
#[inline(always)]
pub(crate) fn bound_slot_meta(raw: usize) -> usize {
    let bound = if raw & SLOT_ACTIVE == 0 { raw } else { 0 };
    bound | SLOT_ACTIVE
}

/// Shade a value that was just stored into a shadow-stack root slot, if any
/// incremental mark cycle is in flight.
///
/// # Why the gate is not a narrowing
///
/// [`crate::gc::runtime_write_barrier_root_nanbox`] is an incremental-marking
/// (root shading) barrier, **not** the generational remembered-set barrier —
/// old→young edges are logged by the heap-slot barriers
/// (`runtime_write_barrier_slot` and friends), which this change does not
/// touch. Its whole body is `incremental_mark_barrier_value`, which reads the
/// thread-local `INCREMENTAL_MARK_BARRIER_VALID_PTRS` and returns immediately
/// when it is null.
///
/// `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` counts the threads whose
/// thread-local pointer is currently non-null. `incremental_mark_barrier_enable`
/// increments the count *before* installing the thread-local; disable clears
/// the thread-local *before* decrementing the count. Thus, on any thread:
///
/// > this thread's `VALID_PTRS` is non-null  ⟹  the count is ≥ 1
///
/// Therefore a zero count proves this thread's pointer is null, i.e. proves
/// the call would have returned `false` without doing anything. Skipping it is
/// observationally identical, not a weaker barrier. A non-zero count is
/// conservative in the harmless direction: another thread's cycle makes us
/// take the call, which then observes its own null pointer and returns.
///
/// This is the same gate codegen already emits inline around
/// `js_write_barrier_root_nanbox` for persistent shadow slots
/// (`perry-codegen/src/expr/shadow_slot.rs::emit_persistent_shadow_root_barrier`);
/// this makes the runtime entry points agree with it.
#[inline(always)]
fn root_shading_barrier(value_bits: u64) {
    if !crate::gc::incremental_mark_barrier_globally_idle() {
        shade_root_slot_value(value_bits);
    }
}

#[cold]
#[inline(never)]
fn shade_root_slot_value(value_bits: u64) {
    crate::gc::runtime_write_barrier_root_nanbox(value_bits);
}

/// Push a new shadow-stack frame with `slot_count` live-pointer
/// slots. Slots start zero-initialized (codegen fills them with
/// NaN-boxed pointer values via `js_shadow_slot_set` /
/// `js_shadow_slot_bind`). Returns an opaque `frame_handle` (the pre-push
/// buffer length) that the matching pop must be passed — lets the GC assert
/// frame balance in debug builds and detects codegen misemission.
///
/// The zero-fill is load-bearing, not hygiene: the buffer beyond the current
/// length still holds the *previous* frame's entries, so a frame that
/// inherited them would report dead slot values — stale addresses of
/// already-freed objects, or live bindings into a stack frame that has since
/// returned — as roots.
#[no_mangle]
pub extern "C" fn js_shadow_frame_push(slot_count: u32) -> u64 {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        push_frame(s, slot_count)
    })
}

/// The body of [`js_shadow_frame_push`], factored out so
/// [`js_shadow_frame_enter`] shares it exactly rather than reimplementing it.
///
/// # Safety
/// `s` must be the calling thread's shadow state.
#[inline(always)]
unsafe fn push_frame(s: &mut ShadowStackState, slot_count: u32) -> u64 {
    let base = s.len;
    let need = SHADOW_STACK_HEADER_SLOTS + slot_count as usize;
    if frame_zero_span(slot_count as usize) > s.cap - base {
        grow_for(s, frame_zero_span(slot_count as usize));
    }
    let header = s.ptr.add(base);
    std::ptr::write(
        header,
        ShadowEntry {
            value: s.frame_top as u64,
            bound: slot_count as usize,
        },
    );
    clear_slots(header.add(SHADOW_STACK_HEADER_SLOTS), slot_count as usize);
    s.set_slots_len(base + need);
    s.frame_top = base + SHADOW_STACK_HEADER_SLOTS;
    base as u64
}

/// [`js_shadow_frame_push`], but returning the address of this thread's
/// [`ShadowStackState`] instead of the frame handle.
///
/// Generated code calls this once per activation and keeps the pointer for the
/// whole frame, so the inline slot stores (#7088) are address arithmetic
/// against it rather than one `extern "C"` call — and one thread-local
/// lookup — per store. The frame handle the matching
/// [`js_shadow_frame_pop`] needs is recoverable without a second call:
/// `handle == frame_top - SHADOW_STACK_HEADER_SLOTS`.
///
/// # Why caching the returned pointer is sound
///
/// It is the address of a `thread_local!` whose type is drop-free and
/// `const`-initialized: fixed for the lifetime of the thread, allocated by the
/// TLS runtime before this function can return it, and never reallocated. The
/// *buffer* it points at does move (see [`grow_for`]), which is exactly why
/// only the state address is cached and `ptr`/`len`/`frame_top` are re-loaded
/// at every store.
///
/// A cached pointer never outlives its thread: one activation of a compiled
/// function runs entirely on one thread (`perry/thread` hands a whole call to
/// a worker; a resumed async state machine re-enters through the function
/// entry and calls this again), so the pointer is refetched whenever the
/// executing thread can have changed.
///
/// Returns a non-null pointer; the declaration codegen emits marks it as such.
#[no_mangle]
pub extern "C" fn js_shadow_frame_enter(slot_count: u32) -> *mut ShadowStackState {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        push_frame(s, slot_count);
        s as *mut ShadowStackState
    })
}

/// The address of this thread's [`ShadowStackState`], without pushing a frame.
///
/// Used by generated code for functions that need inline slot access but whose
/// frame was pushed elsewhere, and by tests that exercise the inline addressing
/// contract from Rust.
#[no_mangle]
pub extern "C" fn js_shadow_state_addr() -> *mut ShadowStackState {
    SHADOW.with(|cell| cell.get())
}

/// Pop the current shadow-stack frame. `frame_handle` must match
/// the return value of the matching `js_shadow_frame_push`. Restores
/// the prior `SHADOW.frame_top`.
///
/// Robustness: the bounds check below was previously a `debug_assert!`,
/// which is **compiled out in release builds**. A corrupted / out-of-range
/// `frame_handle` therefore reached the header entry unchecked and aborted
/// the entire process with an out-of-bounds panic. This was observed on
/// Windows release builds, where codegen could thread a NaN-boxed value
/// (e.g. boxed `undefined`, `0x7FFC_0000_0000_0001`) into this `extern "C"`
/// argument instead of the small index `js_shadow_frame_push` returned —
/// `js_shadow_frame_pop(9222246136947933185)` → out-of-range header read →
/// hard crash a few seconds into startup. Skipping a malformed pop is
/// memory-safe and GC-correctness-neutral (it leaves the frame installed,
/// which over-approximates the root set); aborting the host program is not.
#[no_mangle]
pub extern "C" fn js_shadow_frame_pop(frame_handle: u64) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let base = frame_handle as usize;
        // `base >= len`, not `base + HEADER_SLOTS > len`: the addition form
        // wraps for a handle near `usize::MAX` and lets exactly the corrupted
        // handles this guard exists for slip through into an unchecked read.
        if base >= s.len {
            report_corrupt_frame_pop(frame_handle);
            return;
        }
        s.frame_top = (*s.ptr.add(base)).value as usize;
        // `ShadowEntry: Copy`, so shrinking has no drop glue to run.
        s.set_slots_len(base);
    });
}

/// Report an out-of-range `js_shadow_frame_pop` handle without unwinding.
///
/// #7145: this guard used to be `debug_assert!(false, …)`. That is unsound
/// *inside an `extern "C"` fn*: in a debug build the assert fires, the panic
/// cannot unwind across the `extern "C"` boundary, and the process aborts.
/// `gc::tests::shadow_stack_ops::out_of_range_frame_pop_is_ignored` calls this
/// path deliberately, so `cargo test -p perry-runtime --lib` in the dev profile
/// SIGABRTed the entire test binary — a whole profile of the runtime suite was
/// un-runnable, and CI could not see it because CI runs `--release`, where
/// `debug_assert!` compiles out.
///
/// Skipping the malformed pop is the documented behaviour in both profiles now;
/// the diagnostic is one line on stderr, emitted once per process so it cannot
/// perturb a stdout parity comparison (the
/// `report_growth_stub_skipped_below_heap_min` pattern).
#[cold]
fn report_corrupt_frame_pop(frame_handle: u64) {
    use std::sync::atomic::AtomicBool;
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "[perry-gc] shadow-stack pop past end: frame handle {frame_handle:#x} is out of \
range; the pop was SKIPPED (memory-safe, over-approximates the root set). This \
usually means a caller threaded a NaN-boxed value into js_shadow_frame_pop \
instead of the index js_shadow_frame_push returned. Reported once per process."
    );
}

/// Update slot `idx` in the current frame with `value`.
/// Codegen emits this at safepoints for each live pointer-typed
/// local, and for the `value = 0` "local is dead from here" clear.
///
/// # Slot value contract (#6910)
///
/// A slot may hold a heap reference either **NaN-boxed** (`POINTER_TAG` /
/// `STRING_TAG` / `BIGINT_TAG`, what all shipped codegen emits — "tagged at
/// rest") or **bare** (the untagged user address, what the
/// representation-selection RFC §5.6 unboxed pointer reps are designed to
/// store). Both are marked and both are rewritten, because both mark
/// (`mark_mutable_root_bits`) and rewrite (`try_rewrite_value`) decode
/// through `gc::root_words::decode_root_word`. If you ever add a third
/// in-slot representation, teach that decoder — never one path only.
///
/// What a slot must NOT hold is an *interior* pointer: root marking resolves
/// object starts, so a derived address (typed-array data pointer, `arr + 8`)
/// is only safe region-locally between safepoints and must be recomputed
/// from the rewritten header after each one.
#[no_mangle]
pub extern "C" fn js_shadow_slot_set(idx: u32, value: u64) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let top = s.frame_top;
        if top == usize::MAX {
            return; // no frame active — no-op
        }
        let slot = top + idx as usize;
        if slot >= s.len {
            return;
        }
        let entry = s.ptr.add(slot);
        let meta = (*entry).bound;
        (*entry).value = value;
        if value == 0 {
            // Codegen's "dead from here" clear: drop the liveness bit but keep
            // the binding, so a later re-activation still writes through to the
            // same compiled local slot.
            (*entry).bound = meta & SLOT_PTR_MASK;
            return;
        }
        (*entry).bound = meta | SLOT_ACTIVE;
        root_shading_barrier(value);
        let bound = (meta & SLOT_PTR_MASK) as *mut u64;
        if !bound.is_null() {
            *bound = value;
        }
    });
}

/// Bind shadow slot `idx` to the actual compiled local slot that generated code
/// will read after safepoints. Copied-minor GC can then rewrite the real local
/// alloca instead of only updating the shadow mirror.
#[no_mangle]
pub extern "C" fn js_shadow_slot_bind(idx: u32, value_slot: *mut u64) {
    if value_slot.is_null() {
        return;
    }
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let top = s.frame_top;
        if top == usize::MAX {
            return;
        }
        let slot = top + idx as usize;
        if slot >= s.len {
            return;
        }
        // Snapshot what the mutator has in the slot right now, and root that
        // exact word. Never re-read it later at a safepoint: a re-read can
        // observe a *subsequent* store and root the wrong value.
        let value = *value_slot;
        let raw = value_slot as usize;
        debug_assert_eq!(
            raw & SLOT_ACTIVE,
            0,
            "bound compiled local slot must be 8-byte aligned"
        );
        std::ptr::write(
            s.ptr.add(slot),
            ShadowEntry {
                value,
                bound: bound_slot_meta(raw),
            },
        );
        root_shading_barrier(value);
    });
}

/// Read the current frame's slot `idx` — test-only; the GC tracer walks the
/// raw buffer directly instead of going through a function call per slot.
#[no_mangle]
pub extern "C" fn js_shadow_slot_get(idx: u32) -> u64 {
    SHADOW.with(|cell| unsafe {
        let s = &*cell.get();
        let top = s.frame_top;
        if top == usize::MAX {
            return 0;
        }
        let slot = top + idx as usize;
        let Some(entry) = s.slots().get(slot).copied() else {
            return 0;
        };
        if !entry.is_active() {
            return 0;
        }
        let bound = entry.bound_ptr();
        if bound.is_null() {
            entry.value
        } else {
            *bound
        }
    })
}

/// Current frame depth — test-only.
pub fn shadow_stack_depth() -> usize {
    SHADOW.with(|cell| unsafe {
        let s = &*cell.get();
        // Count frames by walking prev_frame_top pointers from the
        // top back to the bottom. Depth = number of hops to reach
        // `usize::MAX`.
        let mut top = s.frame_top;
        let mut depth = 0;
        while top != usize::MAX && top >= SHADOW_STACK_HEADER_SLOTS {
            depth += 1;
            let header_base = top - SHADOW_STACK_HEADER_SLOTS;
            if header_base >= s.len {
                break;
            }
            top = s.slots()[header_base].value as usize;
        }
        depth
    })
}

pub(crate) fn shadow_stack_has_active_frame() -> bool {
    SHADOW.with(|cell| unsafe { (*cell.get()).frame_top != usize::MAX })
}

/// A snapshot of the shadow stack's depth at a point in time (issue #1830).
///
/// A throw skips the `js_shadow_frame_pop` every abandoned compiled frame
/// would have run before its `ret`. The shadow stack's `frame_top` would
/// then keep pointing at the deepest (now-dead) callee frame, and any GC that
/// scans roots (`visit_shadow_stack_root_slots`) would walk those orphaned
/// frames, reading — and, on the copying/evacuating path, *writing back
/// into* — bound slot pointers into stack memory that the catch body reuses.
///
/// A shadow frame is restored by the frame that holds its handle: a compiled
/// frame's landing pad calls [`js_shadow_frame_restore_to`] with its own
/// handle, and a runtime trap (`crate::exception`) keeps this snapshot of the
/// stack it was entered with. The temp-root stack is not part of it: its
/// slots carry frame marks (`crate::exception::marks`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrameRootSavepoint {
    pub(crate) frame_top: usize,
    pub(crate) len: usize,
}

/// Capture the current shadow-stack depth.
#[inline]
pub(crate) fn frame_root_savepoint() -> FrameRootSavepoint {
    SHADOW.with(|cell| unsafe {
        let s = &*cell.get();
        FrameRootSavepoint {
            frame_top: s.frame_top,
            len: s.len,
        }
    })
}

/// Restore the shadow stack to a previously-captured savepoint, dropping
/// any frames pushed after it. A throw can only have *added* frames
/// relative to the savepoint (the protected region runs strictly deeper),
/// so the saved length is `<=` the current length in the well-formed case;
/// the guard is defensive. `frame_top` is always reset because it is the
/// value the scanner reads.
pub(crate) fn frame_root_restore(sp: FrameRootSavepoint) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        if sp.len <= s.len {
            s.set_slots_len(sp.len);
        }
        s.frame_top = sp.frame_top;
    });
}

/// A compiled frame's landing pad: make the frame whose handle (the value
/// `js_shadow_frame_push` returned) is `frame_handle` the top frame again,
/// dropping every frame its abandoned callees pushed. Idempotent. A
/// malformed handle is skipped, like [`js_shadow_frame_pop`] skips one.
#[no_mangle]
pub extern "C" fn js_shadow_frame_restore_to(frame_handle: u64) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let base = frame_handle as usize;
        if base >= s.len {
            return;
        }
        let slots = (*s.ptr.add(base)).bound;
        let end = base + SHADOW_STACK_HEADER_SLOTS + slots;
        if end <= s.len {
            s.set_slots_len(end);
        }
        s.frame_top = base + SHADOW_STACK_HEADER_SLOTS;
    });
}
