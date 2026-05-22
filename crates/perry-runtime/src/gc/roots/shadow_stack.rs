pub const SHADOW_STACK_HEADER_SLOTS: usize = 2; // prev_frame_top + slot_count
pub const SHADOW_STACK_GROW_RESERVE: usize = 1024; // initial capacity (slots)

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
    /// `Vec<u64>` instead of `Vec<*mut u8>` because slots hold
    /// NaN-boxed JSValue bits (upper 16 bits are the tag, lower 48
    /// the pointer) — the GC tracer unwraps the NaN-box the same way
    /// it already does for closure captures.
    pub(crate) stack: Vec<u64>,
    /// Optional pointer to the compiled local/global slot represented by
    /// each shadow-stack entry. When present, the GC reads and rewrites the
    /// original slot, not a stale mirror copy.
    pub(crate) slot_ptrs: Vec<usize>,
    /// Liveness bit for each shadow slot. This lets codegen stop reporting a
    /// dead local without mutating the compiled local slot after last use.
    pub(crate) active: Vec<bool>,
    /// Index into `stack` where the current frame's slot_0 lives.
    /// `usize::MAX` when no frame is pushed (initial state + after
    /// the outermost function returns).
    pub(crate) frame_top: usize,
}

thread_local! {
    pub(crate) static SHADOW: std::cell::UnsafeCell<ShadowStackState> =
        std::cell::UnsafeCell::new(ShadowStackState {
            stack: Vec::with_capacity(SHADOW_STACK_GROW_RESERVE),
            slot_ptrs: Vec::with_capacity(SHADOW_STACK_GROW_RESERVE),
            active: Vec::with_capacity(SHADOW_STACK_GROW_RESERVE),
            frame_top: usize::MAX,
        });
}

/// Push a new shadow-stack frame with `slot_count` live-pointer
/// slots. Slots start zero-initialized (codegen fills them with
/// NaN-boxed pointer values via `js_shadow_slot_set`). Returns an
/// opaque `frame_handle` (the pre-push top index) that the matching
/// pop must be passed — lets the GC assert frame balance in debug
/// builds and detects codegen misemission.
///
/// Not marked `#[inline(always)]` because it's called once per
/// function entry; the 3-line body inlines naturally.
#[no_mangle]
pub extern "C" fn js_shadow_frame_push(slot_count: u32) -> u64 {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let prev_top = s.frame_top;
        let base = s.stack.len();
        // Header: prev_frame_top + slot_count. Slots follow,
        // initialized to 0 (GC_FLAG_NONE + null pointer).
        s.stack.push(prev_top as u64);
        s.stack.push(slot_count as u64);
        let slots_start = s.stack.len();
        s.stack.resize(slots_start + slot_count as usize, 0);
        s.slot_ptrs.resize(s.stack.len(), 0);
        s.active.resize(s.stack.len(), false);
        s.frame_top = slots_start;
        base as u64
    })
}

/// Pop the current shadow-stack frame. `frame_handle` must match
/// the return value of the matching `js_shadow_frame_push`. Restores
/// the prior `SHADOW.frame_top`.
///
/// Robustness: the bounds check below was previously a `debug_assert!`,
/// which is **compiled out in release builds**. A corrupted / out-of-range
/// `frame_handle` therefore reached `s.stack[base]` unchecked and aborted
/// the entire process with an out-of-bounds panic. This was observed on
/// Windows release builds, where codegen could thread a NaN-boxed value
/// (e.g. boxed `undefined`, `0x7FFC_0000_0000_0001`) into this `extern "C"`
/// argument instead of the small index `js_shadow_frame_push` returned —
/// `js_shadow_frame_pop(9222246136947933185)` → `s.stack[huge]` →
/// hard crash a few seconds into startup. The shadow stack is Phase A
/// (built but not yet consumed by the GC tracer), so skipping a malformed
/// pop is memory-safe and GC-correctness-neutral; aborting the host
/// program is not. Promote the check to a real release-safe guard and
/// bail out — mirrors the bounds checks `js_shadow_slot_set` /
/// `js_shadow_slot_get` already perform on every access.
#[no_mangle]
pub extern "C" fn js_shadow_frame_pop(frame_handle: u64) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let base = frame_handle as usize;
        if base + SHADOW_STACK_HEADER_SLOTS > s.stack.len() {
            debug_assert!(false, "shadow-stack pop past end (corrupted frame handle)");
            return;
        }
        let prev_top = s.stack[base] as usize;
        s.stack.truncate(base);
        s.slot_ptrs.truncate(base);
        s.active.truncate(base);
        s.frame_top = prev_top;
    });
}

/// Update slot `idx` in the current frame with NaN-boxed `value`.
/// Codegen emits this at safepoints for each live pointer-typed
/// local. Hot path — compiled code calls this directly or inlines
/// an equivalent sequence; Rust version exists for runtime tests
/// and debug builds.
#[no_mangle]
pub extern "C" fn js_shadow_slot_set(idx: u32, value: u64) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let top = s.frame_top;
        if top == usize::MAX {
            return; // no frame active — no-op
        }
        let slot = top + idx as usize;
        if slot < s.stack.len() {
            s.stack[slot] = value;
            s.active[slot] = value != 0;
            if value != 0 {
                let ptr = s.slot_ptrs[slot] as *mut u64;
                if !ptr.is_null() {
                    *ptr = value;
                }
            }
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
        if slot < s.stack.len() {
            s.slot_ptrs[slot] = value_slot as usize;
            s.stack[slot] = *value_slot;
            s.active[slot] = true;
        }
    });
}

/// Read the current frame's slot `idx` — test-only; Phase B GC
/// tracer walks the raw Vec directly instead of going through a
/// function call per slot.
#[no_mangle]
pub extern "C" fn js_shadow_slot_get(idx: u32) -> u64 {
    SHADOW.with(|cell| unsafe {
        let s = &*cell.get();
        let top = s.frame_top;
        if top == usize::MAX {
            return 0;
        }
        let slot = top + idx as usize;
        if slot < s.stack.len() {
            if !s.active[slot] {
                return 0;
            }
            let ptr = s.slot_ptrs[slot] as *const u64;
            if ptr.is_null() {
                s.stack[slot]
            } else {
                *ptr
            }
        } else {
            0
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
            if header_base >= s.stack.len() {
                break;
            }
            top = s.stack[header_base] as usize;
        }
        depth
    })
}

pub(crate) fn shadow_stack_has_active_frame() -> bool {
    SHADOW.with(|cell| unsafe { (*cell.get()).frame_top != usize::MAX })
}
