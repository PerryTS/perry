use super::*;

/// Scoped owner for transient runtime handles.
///
/// Handles are mutable GC roots for values that live only in a runtime
/// helper's local variables while that helper may allocate. Dropping the
/// scope removes every handle created from it.
pub struct RuntimeHandleScope {
    pub(super) base: usize,
}

impl RuntimeHandleScope {
    #[inline]
    pub fn new() -> Self {
        let base = RUNTIME_HANDLE_STACK.with(|stack| stack.borrow().len());
        Self { base }
    }

    #[inline]
    pub(super) fn push<'scope>(&'scope self, slot: RuntimeHandleSlot) -> RuntimeHandle<'scope> {
        runtime_handle_slot_write_barrier(slot);
        let index = RUNTIME_HANDLE_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let index = stack.len();
            stack.push(slot);
            index
        });
        RuntimeHandle {
            index,
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
        self.push(RuntimeHandleSlot::RawTagged {
            addr: ptr as usize,
            tag: POINTER_TAG,
        })
    }

    #[inline]
    pub fn root_raw_const_ptr<'scope, T>(&'scope self, ptr: *const T) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::RawTagged {
            addr: ptr as usize,
            tag: POINTER_TAG,
        })
    }

    pub fn root_string_ptr<'scope>(
        &'scope self,
        ptr: *const crate::StringHeader,
    ) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::RawTagged {
            addr: ptr as usize,
            tag: STRING_TAG,
        })
    }

    pub fn root_bigint_ptr<'scope, T>(&'scope self, ptr: *const T) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::RawTagged {
            addr: ptr as usize,
            tag: BIGINT_TAG,
        })
    }

    #[cfg(test)]
    pub(crate) fn active_len_for_tests() -> usize {
        RUNTIME_HANDLE_STACK.with(|stack| stack.borrow().len())
    }
}

/// Snapshot the transient-handle stack before a callback may throw across
/// Rust frames. `longjmp` skips `RuntimeHandleScope::drop`, so exception
/// unwinding restores this depth explicitly.
pub(crate) fn runtime_handle_stack_savepoint() -> usize {
    RUNTIME_HANDLE_STACK.with(|stack| stack.borrow().len())
}

/// Discard transient roots owned by Rust frames skipped by a JS exception.
pub(crate) fn runtime_handle_stack_restore(savepoint: usize) {
    RUNTIME_HANDLE_STACK.with(|stack| stack.borrow_mut().truncate(savepoint));
}

#[inline]
fn runtime_handle_slot_write_barrier(slot: RuntimeHandleSlot) {
    match slot {
        RuntimeHandleSlot::Nanbox(bits) => runtime_write_barrier_root_nanbox(bits),
        RuntimeHandleSlot::HeapWord(bits) => runtime_write_barrier_root_heap_word(bits),
        RuntimeHandleSlot::RawTagged { addr, tag } => {
            if addr != 0 {
                runtime_write_barrier_root_nanbox(tag | (addr as u64 & POINTER_MASK));
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
        RUNTIME_HANDLE_STACK.with(|stack| {
            stack.borrow_mut().truncate(self.base);
        });
    }
}

#[derive(Clone, Copy)]
pub struct RuntimeHandle<'scope> {
    pub(super) index: usize,
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

impl<'scope> RuntimeHandle<'scope> {
    #[inline]
    pub(super) fn with_slot<R>(&self, f: impl FnOnce(RuntimeHandleSlot) -> R) -> R {
        RUNTIME_HANDLE_STACK.with(|stack| {
            let stack = stack.borrow();
            let slot = match stack.get(self.index) {
                Some(slot) => *slot,
                None => handle_used_after_scope(),
            };
            f(slot)
        })
    }

    #[inline]
    pub(super) fn with_slot_mut<R>(&self, f: impl FnOnce(&mut RuntimeHandleSlot) -> R) -> R {
        RUNTIME_HANDLE_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let slot = match stack.get_mut(self.index) {
                Some(slot) => slot,
                None => handle_used_after_scope(),
            };
            f(slot)
        })
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
    /// (*obj).field_count                                        // from-space
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
    /// (*obj).field_count                                        // post-collection
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
            RuntimeHandleSlot::RawTagged { addr, .. } => addr as *mut T,
            _ => handle_kind_mismatch("raw pointer"),
        })
    }

    pub fn set_raw_mut_ptr<T>(&self, ptr: *mut T) {
        self.with_slot_mut(|slot| match slot {
            RuntimeHandleSlot::RawTagged { addr, tag } => {
                *addr = ptr as usize;
                if !ptr.is_null() {
                    runtime_write_barrier_root_nanbox(*tag | (ptr as u64 & POINTER_MASK));
                }
            }
            _ => panic!("runtime handle kind mismatch: expected raw pointer"),
        });
    }

    #[inline]
    pub fn get_raw_const_ptr<T>(&self) -> *const T {
        self.with_slot(|slot| match slot {
            RuntimeHandleSlot::RawTagged { addr, .. } => addr as *const T,
            _ => handle_kind_mismatch("raw pointer"),
        })
    }

    pub fn set_raw_const_ptr<T>(&self, ptr: *const T) {
        self.with_slot_mut(|slot| match slot {
            RuntimeHandleSlot::RawTagged { addr, tag } => {
                *addr = ptr as usize;
                if !ptr.is_null() {
                    runtime_write_barrier_root_nanbox(*tag | (ptr as u64 & POINTER_MASK));
                }
            }
            _ => panic!("runtime handle kind mismatch: expected raw pointer"),
        });
    }
}

pub(crate) fn scan_runtime_handle_roots_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    RUNTIME_HANDLE_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        for slot in stack.iter_mut() {
            match slot {
                RuntimeHandleSlot::Nanbox(bits) => {
                    visitor.visit_nanbox_u64_slot(bits);
                }
                RuntimeHandleSlot::RawTagged { addr, tag } => {
                    visitor.visit_tagged_usize_slot(addr, *tag);
                }
                RuntimeHandleSlot::HeapWord(bits) => {
                    visitor.visit_heap_word_u64_slot(bits);
                }
            }
        }
    });
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
    RUNTIME_HANDLE_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        while *remaining > 0 && state.cursor < stack.len() {
            match &mut stack[state.cursor] {
                RuntimeHandleSlot::Nanbox(bits) => {
                    visitor.visit_nanbox_u64_slot(bits);
                }
                RuntimeHandleSlot::RawTagged { addr, tag } => {
                    visitor.visit_tagged_usize_slot(addr, *tag);
                }
                RuntimeHandleSlot::HeapWord(bits) => {
                    visitor.visit_heap_word_u64_slot(bits);
                }
            }
            state.cursor += 1;
            *remaining -= 1;
        }
        state.cursor >= stack.len()
    })
}
