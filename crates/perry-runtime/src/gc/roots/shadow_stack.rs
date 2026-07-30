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

/// Entries a frame reserves for its header. One [`ShadowEntry`] carries both
/// header words: `value` = the caller's `frame_top`, `meta` = this frame's
/// slot count.
pub const SHADOW_STACK_HEADER_SLOTS: usize = 1;
/// Entries the backing buffer reserves the first time it grows.
pub const SHADOW_STACK_GROW_RESERVE: usize = 1024;

/// Liveness bit, stored in bit 0 of [`ShadowEntry::meta`].
///
/// A bound slot pointer is the address of a compiled `i64`/`double` local
/// slot and is therefore 8-byte aligned, so bit 0 is always free to carry the
/// liveness flag alongside it. [`js_shadow_slot_bind`] refuses to record a
/// pointer that would collide with the tag rather than truncating one.
pub(crate) const SLOT_ACTIVE: usize = 1;
/// Mask recovering the bound compiled-local address from `meta`.
pub(crate) const SLOT_PTR_MASK: usize = !SLOT_ACTIVE;

/// One shadow-stack entry.
///
/// `#[repr(C)]` with `value` first is load-bearing: the GC hands the visitor
/// `&mut entry.value` as a `*mut u64` root slot for unbound entries, so the
/// mirrored word must sit at offset 0 and be 8-byte aligned. 16 bytes also
/// makes indexing a shift rather than a multiply.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ShadowEntry {
    /// The heap word the mutator stored.
    ///
    /// Raw bits rather than a typed pointer because slots hold NaN-boxed
    /// JSValue bits (upper 16 bits are the tag, lower 48 the pointer) — the
    /// GC tracer unwraps the NaN-box the same way it already does for closure
    /// captures.
    pub(crate) value: u64,
    /// `bound_slot_address | SLOT_ACTIVE`.
    ///
    /// The address half is the compiled local/global slot this entry mirrors,
    /// or 0 when the entry is unbound. When present, the GC reads and rewrites
    /// the original slot, not the stale mirror copy. The `SLOT_ACTIVE` bit is
    /// the liveness flag: it lets codegen stop reporting a dead local without
    /// mutating the compiled local slot after last use.
    pub(crate) meta: usize,
}

impl ShadowEntry {
    pub(crate) const EMPTY: ShadowEntry = ShadowEntry { value: 0, meta: 0 };

    #[inline(always)]
    pub(crate) fn is_active(self) -> bool {
        self.meta & SLOT_ACTIVE != 0
    }

    /// The compiled local slot this entry mirrors, or null when unbound.
    #[inline(always)]
    pub(crate) fn bound_ptr(self) -> *mut u64 {
        (self.meta & SLOT_PTR_MASK) as *mut u64
    }
}

/// Combined shadow-stack state. Holding both fields in one TLS slot
/// halves the macOS `tlv_get_addr` calls in every shadow-stack op
/// (push / pop / slot_set / slot_get / scanner) — those ops fired
/// ~3 M+ times per perf-comprehensive run, and TLS access was the
/// single biggest leaf cost in the post-iter-3 profile (20.9 % leaf
/// samples on `tlv_get_addr`). Replacing `RefCell<Vec<u64>>` with
/// `UnsafeCell<ShadowStackState>` also drops the per-op RefCell
/// borrow accounting.
///
/// Safety: shadow-stack ops are only invoked from compiled JS code
/// (runtime-generated, single-threaded for this TLS) and from GC
/// scanner / rewriter passes. The two never overlap — GC is
/// stop-the-world relative to this TLS, and compiled code can't
/// re-enter the runtime through a path that would touch this state
/// while a GC walk is in progress (no allocation occurs inside the
/// scanner/rewriter, and `GC_FLAG_IN_ALLOC` blocks reentrant GC).
pub(crate) struct ShadowStackState {
    /// Every frame's header + slots, back to back.
    pub(crate) slots: Vec<ShadowEntry>,
    /// Index into `slots` where the current frame's slot 0 lives.
    /// `usize::MAX` when no frame is pushed (initial state + after
    /// the outermost function returns).
    pub(crate) frame_top: usize,
}

thread_local! {
    /// `const`-initialized so the access is a plain TLS address computation.
    /// The buffer is reserved lazily on the first push instead of eagerly at
    /// thread start.
    pub(crate) static SHADOW: UnsafeCell<ShadowStackState> = const {
        UnsafeCell::new(ShadowStackState {
            slots: Vec::new(),
            frame_top: usize::MAX,
        })
    };
}

/// Reserve room for `need` more entries. Outlined and `#[cold]` so the push
/// fast path stays a capacity compare and a not-taken branch.
#[cold]
#[inline(never)]
fn grow_for(s: &mut ShadowStackState, need: usize) {
    s.slots.reserve(need.max(SHADOW_STACK_GROW_RESERVE));
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

/// Encode a bound compiled-local address into a live [`ShadowEntry::meta`].
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
/// installs the thread-local *and then* increments the count, both before
/// returning to the mutator, so on any thread:
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
    if crate::gc::PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.load(Ordering::SeqCst) != 0 {
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
        let base = s.slots.len();
        let need = SHADOW_STACK_HEADER_SLOTS + slot_count as usize;
        if frame_zero_span(slot_count as usize) > s.slots.capacity() - base {
            grow_for(s, frame_zero_span(slot_count as usize));
        }
        let header = s.slots.as_mut_ptr().add(base);
        std::ptr::write(
            header,
            ShadowEntry {
                value: s.frame_top as u64,
                meta: slot_count as usize,
            },
        );
        clear_slots(header.add(SHADOW_STACK_HEADER_SLOTS), slot_count as usize);
        s.slots.set_len(base + need);
        s.frame_top = base + SHADOW_STACK_HEADER_SLOTS;
        base as u64
    })
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
        if base >= s.slots.len() {
            debug_assert!(false, "shadow-stack pop past end (corrupted frame handle)");
            return;
        }
        s.frame_top = (*s.slots.as_ptr().add(base)).value as usize;
        // `ShadowEntry: Copy`, so shrinking has no drop glue to run.
        s.slots.set_len(base);
    });
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
        if slot >= s.slots.len() {
            return;
        }
        let entry = s.slots.as_mut_ptr().add(slot);
        let meta = (*entry).meta;
        (*entry).value = value;
        if value == 0 {
            // Codegen's "dead from here" clear: drop the liveness bit but keep
            // the binding, so a later re-activation still writes through to the
            // same compiled local slot.
            (*entry).meta = meta & SLOT_PTR_MASK;
            return;
        }
        (*entry).meta = meta | SLOT_ACTIVE;
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
        if slot >= s.slots.len() {
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
            s.slots.as_mut_ptr().add(slot),
            ShadowEntry {
                value,
                meta: bound_slot_meta(raw),
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
        let Some(entry) = s.slots.get(slot).copied() else {
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
            if header_base >= s.slots.len() {
                break;
            }
            top = s.slots[header_base].value as usize;
        }
        depth
    })
}

pub(crate) fn shadow_stack_has_active_frame() -> bool {
    SHADOW.with(|cell| unsafe { (*cell.get()).frame_top != usize::MAX })
}

/// A snapshot of the shadow stack's depth at a point in time. Captured
/// when a `try` block is established and replayed on the exception
/// unwind path (issue #1830).
///
/// Why this exists: exception unwinding uses `longjmp` (see
/// `crate::exception`), which restores the native SP/registers to the
/// `setjmp` site WITHOUT running the epilogues of the functions being
/// unwound past. Those functions emitted `js_shadow_frame_pop` before
/// their `ret`, so on a normal return the shadow stack stays balanced —
/// but a `longjmp` jumps straight over every skipped pop. The shadow
/// stack's `frame_top` is then left pointing at the deepest (now-dead)
/// callee frame. Until the unwinding function eventually returns, any GC
/// that scans roots (`visit_shadow_stack_root_slots`) would walk those
/// orphaned frames, reading — and, on the copying/evacuating path,
/// *writing back into* — bound slot pointers that point into stack memory
/// that has already been unwound and is being reused by the catch body.
///
/// The same reasoning applies to the temp-root stack (#6951): generated code
/// pushes an expression temporary, evaluates something that throws, and never
/// reaches its `js_gc_temp_root_truncate`. That depth is therefore recorded
/// here and restored with the frames, so one savepoint covers both precise
/// root stacks and `crate::exception` needs no separate hook.
#[derive(Copy, Clone)]
pub(crate) struct ShadowSavepoint {
    frame_top: usize,
    len: usize,
    temp_roots: usize,
}

impl ShadowSavepoint {
    /// Identity savepoint for an empty shadow stack — used to
    /// zero-initialize the per-try-depth savepoint table.
    pub(crate) const EMPTY: ShadowSavepoint = ShadowSavepoint {
        frame_top: usize::MAX,
        len: 0,
        temp_roots: 0,
    };
}

/// Capture the current shadow-stack depth so it can be restored after a
/// non-local exit. Call at `js_try_push` time, before the protected
/// region can push any callee frames.
pub(crate) fn shadow_stack_savepoint() -> ShadowSavepoint {
    SHADOW.with(|cell| unsafe {
        let s = &*cell.get();
        ShadowSavepoint {
            frame_top: s.frame_top,
            len: s.slots.len(),
            temp_roots: super::temp_roots::temp_root_depth(),
        }
    })
}

/// Restore the shadow stack to a previously-captured savepoint, dropping
/// any frames pushed after it. Called on the exception unwind path
/// (before `longjmp`) so the orphaned shadow frames of the functions
/// being unwound past are never scanned by a later GC (issue #1830).
///
/// A `longjmp` can only have *added* frames relative to the savepoint
/// (the protected region runs strictly deeper than the `try`), so the
/// saved length is `<=` the current length in the well-formed case. The
/// `<=` guard is purely defensive against a corrupted savepoint; we
/// always reset `frame_top` because it is the value the scanner reads.
pub(crate) fn shadow_stack_restore(sp: ShadowSavepoint) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        if sp.len <= s.slots.len() {
            s.slots.truncate(sp.len);
        }
        s.frame_top = sp.frame_top;
    });
    super::temp_roots::temp_roots_restore(sp.temp_roots);
}
