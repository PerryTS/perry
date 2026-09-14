use super::*;

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
mod stack;
#[cfg(any(target_os = "android", target_env = "ohos"))]
#[path = "runtime_handles/stack_os_tls.rs"]
mod stack;
#[cfg(test)]
mod tests;
use stack::{RuntimeHandleStack, StackRef};

struct RuntimeHandleStackHotGuard;

impl Drop for RuntimeHandleStackHotGuard {
    fn drop(&mut self) {
        crate::tls_hot::unpublish_runtime_handle_stack();
        #[cfg(not(any(target_os = "android", target_env = "ohos")))]
        RUNTIME_HANDLE_STACK.with(RuntimeHandleStack::release);
    }
}

thread_local! {
    /// This stays a raw `thread_local!` as the initialization fallback for its
    /// named `HotTls` slot. Going through `perry_thread_local!` here would call
    /// `hot()` while `HotTls::fill` is still resolving this address (#9183).
    static RUNTIME_HANDLE_STACK: RuntimeHandleStack = const { RuntimeHandleStack::new() };
    /// Unpublishes the cache at teardown. Native TLS registers this owner
    /// before allocation and releases the manual buffer here; OS-backed TLS keeps
    /// the original Vec owner in RUNTIME_HANDLE_STACK instead.
    static RUNTIME_HANDLE_STACK_HOT_GUARD: RuntimeHandleStackHotGuard = const { RuntimeHandleStackHotGuard };
}

/// Resolve this thread's runtime-handle stack without entering `HotTls`.
/// Called once by `tls_hot::fill`, and by handle operations only until the
/// Darwin direct-TSD cache has been published.
#[inline(always)]
pub(crate) fn runtime_handle_stack_hot_addr() -> *mut u8 {
    let addr = RUNTIME_HANDLE_STACK.with(|stack| stack as *const _ as *mut u8);
    RUNTIME_HANDLE_STACK_HOT_GUARD.with(|_| {});
    addr
}

/// Resolve this thread's transient-handle metadata, without initializing HOT.
///
/// On Apple aarch64 the steady-state path reads the address from an already
/// published `HotTls`. During `HotTls::fill` (and on every other target) it
/// uses the ordinary thread-local directly, so opening a handle scope cannot
/// recursively initialize the cache (#9183).
#[inline(always)]
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
fn runtime_handle_stack() -> StackRef {
    if let Some(hot) = crate::tls_hot::hot_if_published() {
        let stack = hot.runtime_handle_stack.get();
        if !stack.is_null() {
            // SAFETY: fill publishes this thread's non-dropping metadata.
            // The buffer owner clears it before freeing the allocation.
            return unsafe { &*(stack as *const RuntimeHandleStack) };
        }
    }
    RUNTIME_HANDLE_STACK.with(|stack| {
        // SAFETY: the metadata is const-initialized and has no Drop. Its
        // cells remain valid throughout thread teardown. Cell is !Sync, so
        // scopes and handles carrying this reference cannot leave the thread.
        unsafe { &*(stack as *const RuntimeHandleStack) }
    })
}

// Rust's Android/HarmonyOS OS-TLS backend frees the metadata even when T has no Drop.
// Keep the original scoped lookup there; a token never borrows TLS storage
// across a callback or a later destructor.
#[cfg(any(target_os = "android", target_env = "ohos"))]
#[inline(always)]
fn runtime_handle_stack() -> StackRef {
    StackRef::new()
}

/// Scoped owner for transient runtime handles.
///
/// Handles are mutable GC roots for values that live only in a runtime
/// helper's local variables while that helper may allocate. Dropping the
/// scope removes every handle created from it.
pub struct RuntimeHandleScope {
    pub(super) base: usize,
    stack: StackRef,
}

impl RuntimeHandleScope {
    #[inline]
    pub fn new() -> Self {
        let stack = runtime_handle_stack();
        Self {
            base: stack.len(),
            stack,
        }
    }

    #[inline]
    pub(super) fn push<'scope>(&'scope self, slot: RuntimeHandleSlot) -> RuntimeHandle<'scope> {
        runtime_handle_slot_write_barrier(slot);
        let index = self.stack.push(slot);
        RuntimeHandle {
            index,
            stack: self.stack,
            _scope: PhantomData,
        }
    }

    #[inline]
    pub fn root_nanbox_f64<'scope>(&'scope self, value: f64) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::Nanbox(value.to_bits()))
    }

    pub fn root_nanbox_f64_slice<'scope>(
        &'scope self,
        values: &[f64],
    ) -> Vec<RuntimeHandle<'scope>> {
        values
            .iter()
            .map(|value| self.root_nanbox_f64(*value))
            .collect()
    }

    #[inline]
    pub fn root_nanbox_u64<'scope>(&'scope self, bits: u64) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::Nanbox(bits))
    }

    pub fn root_heap_word_u64<'scope>(&'scope self, bits: u64) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::HeapWord(bits))
    }

    pub fn root_heap_word_u64_slice<'scope>(
        &'scope self,
        values: &[u64],
    ) -> Vec<RuntimeHandle<'scope>> {
        values
            .iter()
            .map(|bits| self.root_heap_word_u64(*bits))
            .collect()
    }

    pub fn refreshed_nanbox_f64_slice(handles: &[RuntimeHandle<'_>]) -> Vec<f64> {
        handles.iter().map(RuntimeHandle::get_nanbox_f64).collect()
    }

    pub fn refreshed_heap_word_u64_slice(handles: &[RuntimeHandle<'_>]) -> Vec<u64> {
        handles
            .iter()
            .map(RuntimeHandle::get_heap_word_u64)
            .collect()
    }

    #[inline]
    pub fn root_raw_mut_ptr<'scope, T>(&'scope self, ptr: *mut T) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::RawPointer(ptr as usize))
    }

    #[inline]
    pub fn root_raw_const_ptr<'scope, T>(&'scope self, ptr: *const T) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::RawPointer(ptr as usize))
    }

    pub fn root_string_ptr<'scope>(
        &'scope self,
        ptr: *const crate::StringHeader,
    ) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::RawString(ptr as usize))
    }

    pub fn root_bigint_ptr<'scope, T>(&'scope self, ptr: *const T) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::RawBigInt(ptr as usize))
    }

    #[cfg(test)]
    pub(crate) fn active_len_for_tests() -> usize {
        runtime_handle_stack().len()
    }

    #[cfg(test)]
    pub(crate) fn capacity_for_tests() -> usize {
        runtime_handle_stack().capacity()
    }
}

/// Snapshot the transient-handle stack before a callback may throw across
/// Rust frames. `longjmp` skips `RuntimeHandleScope::drop`, so exception
/// unwinding restores this depth explicitly.
#[inline]
pub(crate) fn runtime_handle_stack_savepoint() -> usize {
    runtime_handle_stack().len()
}

/// Discard transient roots owned by Rust frames skipped by a JS exception.
pub(crate) fn runtime_handle_stack_restore(savepoint: usize) {
    runtime_handle_stack().truncate(savepoint);
}

#[inline(always)]
fn runtime_handle_slot_write_barrier(slot: RuntimeHandleSlot) {
    // Root stores shade only during incremental marking. Keep the idle test
    // ahead of kind decoding and the out-of-line active barrier machinery.
    if crate::gc::barrier::incremental_mark_barrier_globally_idle() {
        return;
    }
    runtime_handle_slot_write_barrier_active(slot);
}

#[cold]
#[inline(never)]
fn runtime_handle_slot_write_barrier_active(slot: RuntimeHandleSlot) {
    match slot {
        RuntimeHandleSlot::Nanbox(bits) => runtime_write_barrier_root_nanbox(bits),
        RuntimeHandleSlot::HeapWord(bits) => runtime_write_barrier_root_heap_word(bits),
        RuntimeHandleSlot::RawPointer(addr)
        | RuntimeHandleSlot::RawString(addr)
        | RuntimeHandleSlot::RawBigInt(addr) => {
            if addr != 0 {
                runtime_write_barrier_root_nanbox(
                    raw_slot_tag(slot) | (addr as u64 & POINTER_MASK),
                );
            }
        }
    }
}

impl Default for RuntimeHandleScope {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RuntimeHandleScope {
    #[inline]
    fn drop(&mut self) {
        self.stack.truncate(self.base);
    }
}

#[derive(Clone, Copy)]
pub struct RuntimeHandle<'scope> {
    pub(super) index: usize,
    stack: StackRef,
    pub(super) _scope: PhantomData<&'scope RuntimeHandleScope>,
}

/// The two failure paths every handle accessor carries. Out of line and
/// `#[cold]` so the accessors stay small enough for the inliner: a formatted
/// `expect`/`panic!` expanded inline is most of each accessor's estimated
/// size, and it was enough to keep `get_nanbox_u64` / `root_nanbox_f64` out of
/// line in the release build — they showed up as 4.3 % and 3.0 % of a
/// promise-heavy program's leaf samples purely as call frames.
#[cold]
#[inline(never)]
fn handle_used_after_scope() -> ! {
    panic!("runtime handle used after its scope was dropped");
}

#[cold]
#[inline(never)]
fn handle_kind_mismatch(expected: &str) -> ! {
    panic!("runtime handle kind mismatch: expected {expected}");
}

#[inline]
fn raw_slot_tag(slot: RuntimeHandleSlot) -> u64 {
    match slot {
        RuntimeHandleSlot::RawPointer(_) => POINTER_TAG,
        RuntimeHandleSlot::RawString(_) => STRING_TAG,
        RuntimeHandleSlot::RawBigInt(_) => BIGINT_TAG,
        _ => handle_kind_mismatch("raw pointer"),
    }
}

// Encode the three raw tags in the discriminant, rather than storing a
// separate u64 tag alongside every payload (24 -> 16 bytes on 64-bit hosts).
const _: () = assert!(std::mem::size_of::<RuntimeHandleSlot>() == 16);

impl<'scope> RuntimeHandle<'scope> {
    #[inline]
    pub(super) fn with_slot<R>(&self, f: impl FnOnce(RuntimeHandleSlot) -> R) -> R {
        let slot = match self.stack.get(self.index) {
            Some(slot) => slot,
            None => handle_used_after_scope(),
        };
        f(slot)
    }

    #[inline]
    pub(super) fn with_slot_mut<R>(&self, f: impl FnOnce(&mut RuntimeHandleSlot) -> R) -> R {
        let mut slot = self.with_slot(|slot| slot);
        let result = f(&mut slot);
        self.stack.set(self.index, slot);
        result
    }

    /// Pass the handle's current mutable pointer to `f` without exposing a
    /// bare handle read at the call site.
    ///
    /// This is the argument-position companion to [`Self::across_mut`]. Use it
    /// when a rooted pointer must be handed directly to a non-allocating
    /// operation or to a runtime entry point that establishes its own root
    /// before it can allocate. The callback must not retain the pointer: this
    /// method scopes the raw value, but it cannot keep that value current if a
    /// collection moves the allocation while `f` is running. Use
    /// [`Self::across_mut`] when the caller needs a post-collection address.
    #[inline]
    pub fn with_mut_ptr<T, R>(&self, f: impl FnOnce(*mut T) -> R) -> R {
        f(self.get_raw_mut_ptr::<T>())
    }

    /// `with_mut_ptr` for a `*const` argument. See its safety contract.
    #[inline]
    pub fn with_const_ptr<T, R>(&self, f: impl FnOnce(*const T) -> R) -> R {
        f(self.get_raw_const_ptr::<T>())
    }

    /// Re-read a rooted string's current payload and pass it to a
    /// non-allocating callback.
    ///
    /// Call this again after every operation that may allocate or poll the
    /// collector. A copying collection refreshes this handle's slot, and this
    /// method derives the slice from that refreshed address on every call. Do
    /// not retain the slice or allocate inside `f`; use
    /// [`crate::string::OwnedStringBytes::copy_from_header`] when the bytes must
    /// cross a collection point. `string_copy_range` and the byte-range loop in
    /// `string/split.rs` are the reference patterns: keep offsets, perform the
    /// allocating operation, then re-read the rooted source before touching
    /// its payload.
    ///
    /// # Safety
    ///
    /// This handle must have been created by
    /// [`RuntimeHandleScope::root_string_ptr`] from a live, initialized,
    /// non-null [`crate::StringHeader`]. `f` must not invoke an operation that
    /// can move or free the string while its payload slice is borrowed.
    #[inline]
    pub unsafe fn with_string_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        let ptr = self.with_slot(|slot| match slot {
            RuntimeHandleSlot::RawString(addr) => addr as *const crate::StringHeader,
            _ => handle_kind_mismatch("rooted string pointer"),
        });
        let bytes = unsafe {
            std::slice::from_raw_parts(crate::string::string_data(ptr), (*ptr).byte_len as usize)
        };
        f(bytes)
    }

    /// Run `f` — which may allocate, and therefore may MOVE the object this
    /// handle roots — and return its result together with the object's
    /// **post-collection** address.
    ///
    /// # Why this exists
    ///
    /// `docs/src/internals/gc-rooting-invariant.md` states the rule, and the
    /// second half is the half that keeps getting dropped:
    ///
    /// > A value read out of a root and held in a register across a call is not
    /// > rooted. It is a copy, and the collector cannot see copies.
    ///
    /// A `RuntimeHandleScope` gives an object *liveness*: the collector marks it
    /// and rewrites the slot. It does nothing about a raw pointer already read
    /// out of that slot. Every bug in the #7341 quarantine sweep that was fixed
    /// by rooting had rooting **already** — what was missing was ordering the
    /// re-read relative to the collection point:
    ///
    /// ```ignore
    /// let obj = obj_h.get_raw_mut_ptr::<ObjectHeader>();
    /// let found = class_instance_has_member(class_id, "size");  // ALLOCATES
    /// crate::object::object_live_slot_count(obj)                                        // from-space
    /// ```
    ///
    /// The defect is not a missing root. It is that `obj` is still *nameable*
    /// after the call. This combinator removes that: the pre-call address is
    /// never bound, so there is nothing stale to reach for.
    ///
    /// ```ignore
    /// let (found, obj) = obj_h.across_mut::<ObjectHeader, _>(
    ///     || class_instance_has_member(class_id, "size"),
    /// );
    /// crate::object::object_live_slot_count(obj)                                        // post-collection
    /// ```
    ///
    /// # What it does NOT do
    ///
    /// It is not a proof. It cannot stop you reading the pointer *before* the
    /// call and holding that copy yourself — Rust has no effect system to mark
    /// "this call may allocate", so no signature can reject that. What it does
    /// is make the correct shape shorter than the incorrect one and give the
    /// ratchet in `scripts/raw_handle_debt.py` something to count down.
    #[inline]
    pub fn across_mut<T, R>(&self, f: impl FnOnce() -> R) -> (R, *mut T) {
        let result = f();
        (result, self.get_raw_mut_ptr::<T>())
    }

    /// `across_mut` for a `*const` receiver. See its docs.
    #[inline]
    pub fn across_const<T, R>(&self, f: impl FnOnce() -> R) -> (R, *const T) {
        let result = f();
        (result, self.get_raw_const_ptr::<T>())
    }

    /// `across_mut` for a NaN-boxed value.
    #[inline]
    pub fn across_nanbox<R>(&self, f: impl FnOnce() -> R) -> (R, f64) {
        let result = f();
        (result, self.get_nanbox_f64())
    }

    #[inline]
    pub fn get_nanbox_f64(&self) -> f64 {
        f64::from_bits(self.get_nanbox_u64())
    }

    #[inline]
    pub fn get_nanbox_u64(&self) -> u64 {
        self.with_slot(|slot| match slot {
            RuntimeHandleSlot::Nanbox(bits) => bits,
            _ => handle_kind_mismatch("NaN-boxed value"),
        })
    }

    pub fn set_nanbox_f64(&self, value: f64) {
        self.set_nanbox_u64(value.to_bits());
    }

    #[inline]
    pub fn set_nanbox_u64(&self, bits: u64) {
        self.with_slot_mut(|slot| match slot {
            RuntimeHandleSlot::Nanbox(current) => *current = bits,
            _ => handle_kind_mismatch("NaN-boxed value"),
        });
        runtime_write_barrier_root_nanbox(bits);
    }

    #[inline]
    pub fn get_heap_word_u64(&self) -> u64 {
        self.with_slot(|slot| match slot {
            RuntimeHandleSlot::HeapWord(bits) => bits,
            _ => handle_kind_mismatch("heap word"),
        })
    }

    pub fn set_heap_word_u64(&self, bits: u64) {
        self.with_slot_mut(|slot| match slot {
            RuntimeHandleSlot::HeapWord(current) => *current = bits,
            _ => panic!("runtime handle kind mismatch: expected heap word"),
        });
        runtime_write_barrier_root_heap_word(bits);
    }

    #[inline]
    pub fn get_raw_mut_ptr<T>(&self) -> *mut T {
        self.with_slot(|slot| match slot {
            RuntimeHandleSlot::RawPointer(addr)
            | RuntimeHandleSlot::RawString(addr)
            | RuntimeHandleSlot::RawBigInt(addr) => addr as *mut T,
            _ => handle_kind_mismatch("raw pointer"),
        })
    }

    pub fn set_raw_mut_ptr<T>(&self, ptr: *mut T) {
        self.set_raw_const_ptr(ptr.cast_const());
    }

    #[inline]
    pub fn get_raw_const_ptr<T>(&self) -> *const T {
        self.with_slot(|slot| match slot {
            RuntimeHandleSlot::RawPointer(addr)
            | RuntimeHandleSlot::RawString(addr)
            | RuntimeHandleSlot::RawBigInt(addr) => addr as *const T,
            _ => handle_kind_mismatch("raw pointer"),
        })
    }

    pub fn set_raw_const_ptr<T>(&self, ptr: *const T) {
        let slot = self.with_slot_mut(|slot| {
            match slot {
                RuntimeHandleSlot::RawPointer(addr)
                | RuntimeHandleSlot::RawString(addr)
                | RuntimeHandleSlot::RawBigInt(addr) => *addr = ptr as usize,
                _ => handle_kind_mismatch("raw pointer"),
            }
            *slot
        });
        runtime_handle_slot_write_barrier(slot);
    }
}

/// Visit a copy, then commit a relocation to its indexed slot. No reference
/// into the growable buffer crosses a visitor call; the legacy Copy visitor
/// may invoke arbitrary callbacks. Non-rewriting visits must not overwrite a
/// value changed by such a callback.
fn visit_runtime_handle_slot(stack: StackRef, index: usize, visitor: &mut RuntimeRootVisitor<'_>) {
    let Some(mut slot) = stack.get(index) else {
        return;
    };
    let rewritten = match &mut slot {
        RuntimeHandleSlot::Nanbox(bits) => visitor.visit_nanbox_u64_slot(bits),
        RuntimeHandleSlot::RawPointer(addr) => visitor.visit_tagged_usize_slot(addr, POINTER_TAG),
        RuntimeHandleSlot::RawString(addr) => visitor.visit_tagged_usize_slot(addr, STRING_TAG),
        RuntimeHandleSlot::RawBigInt(addr) => visitor.visit_tagged_usize_slot(addr, BIGINT_TAG),
        RuntimeHandleSlot::HeapWord(bits) => visitor.visit_heap_word_u64_slot(bits),
    };
    if rewritten {
        stack.set(index, slot);
    }
}

pub(crate) fn scan_runtime_handle_roots_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    let stack = runtime_handle_stack();
    for index in 0..stack.len() {
        visit_runtime_handle_slot(stack, index, visitor);
    }
}

#[derive(Default)]
pub(crate) struct RuntimeHandleRootScanState {
    cursor: usize,
}

pub(crate) fn new_runtime_handle_root_scan_state() -> Box<dyn Any> {
    Box::<RuntimeHandleRootScanState>::default()
}

pub(crate) fn scan_runtime_handle_roots_mut_step(
    visitor: &mut RuntimeRootVisitor<'_>,
    state: &mut dyn Any,
    remaining: &mut usize,
) -> bool {
    let state = state
        .downcast_mut::<RuntimeHandleRootScanState>()
        .expect("runtime handle root scanner state type");
    let stack = runtime_handle_stack();
    while *remaining > 0 && state.cursor < stack.len() {
        visit_runtime_handle_slot(stack, state.cursor, visitor);
        state.cursor += 1;
        *remaining -= 1;
    }
    state.cursor >= stack.len()
}

// ---------------------------------------------------------------------------
// FFI transient roots (#8082).
// ---------------------------------------------------------------------------
//
// Extern surface over the transient-handle stack for ext crates (perry-ffi
// consumers). Their handle-struct side tables are rewritten by registered
// mutable-root scanners, but a SNAPSHOT of those tables held in a Rust local
// across a JS callback is a copy the collector cannot see — the #8082 forced
// gate faulted on exactly that shape in the http server's pending-request
// pump. These entry points let FFI code park such copies in slots the
// existing runtime-handle scanner marks AND rewrites, then re-read the
// post-collection values. Scopes must strictly nest: `enter` snapshots the
// depth, `exit` truncates back to it (the JS-exception savepoint machinery
// above already restores this stack across throws).

#[no_mangle]
pub extern "C" fn js_ffi_root_scope_enter() -> usize {
    runtime_handle_stack().len()
}

/// Root a raw heap ADDRESS (e.g. an `i64` closure pointer from an ext
/// listener table). Returns the slot index for [`js_ffi_root_get_heap_addr`].
#[no_mangle]
pub extern "C" fn js_ffi_root_push_heap_addr(addr: u64) -> usize {
    let slot = RuntimeHandleSlot::HeapWord(addr);
    runtime_handle_slot_write_barrier(slot);
    runtime_handle_stack().push(slot)
}

#[no_mangle]
pub extern "C" fn js_ffi_root_get_heap_addr(index: usize) -> u64 {
    match runtime_handle_stack().get(index) {
        Some(RuntimeHandleSlot::HeapWord(bits)) => bits,
        _ => 0,
    }
}

/// Root a NaN-boxed VALUE (string/buffer/object handed to callbacks).
#[no_mangle]
pub extern "C" fn js_ffi_root_push_nanbox(bits: u64) -> usize {
    let slot = RuntimeHandleSlot::Nanbox(bits);
    runtime_handle_slot_write_barrier(slot);
    runtime_handle_stack().push(slot)
}

#[no_mangle]
pub extern "C" fn js_ffi_root_get_nanbox(index: usize) -> u64 {
    match runtime_handle_stack().get(index) {
        Some(RuntimeHandleSlot::Nanbox(bits)) => bits,
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn js_ffi_root_scope_exit(base: usize) {
    runtime_handle_stack().truncate(base);
}
