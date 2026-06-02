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

    pub fn root_raw_mut_ptr<'scope, T>(&'scope self, ptr: *mut T) -> RuntimeHandle<'scope> {
        self.push(RuntimeHandleSlot::RawTagged {
            addr: ptr as usize,
            tag: POINTER_TAG,
        })
    }

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

impl<'scope> RuntimeHandle<'scope> {
    #[inline]
    pub(super) fn with_slot<R>(&self, f: impl FnOnce(RuntimeHandleSlot) -> R) -> R {
        RUNTIME_HANDLE_STACK.with(|stack| {
            let stack = stack.borrow();
            let slot = *stack
                .get(self.index)
                .expect("runtime handle used after its scope was dropped");
            f(slot)
        })
    }

    #[inline]
    pub(super) fn with_slot_mut<R>(&self, f: impl FnOnce(&mut RuntimeHandleSlot) -> R) -> R {
        RUNTIME_HANDLE_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let slot = stack
                .get_mut(self.index)
                .expect("runtime handle used after its scope was dropped");
            f(slot)
        })
    }

    pub fn get_nanbox_f64(&self) -> f64 {
        f64::from_bits(self.get_nanbox_u64())
    }

    pub fn get_nanbox_u64(&self) -> u64 {
        self.with_slot(|slot| match slot {
            RuntimeHandleSlot::Nanbox(bits) => bits,
            _ => panic!("runtime handle kind mismatch: expected NaN-boxed value"),
        })
    }

    pub fn set_nanbox_f64(&self, value: f64) {
        self.set_nanbox_u64(value.to_bits());
    }

    pub fn set_nanbox_u64(&self, bits: u64) {
        self.with_slot_mut(|slot| match slot {
            RuntimeHandleSlot::Nanbox(current) => *current = bits,
            _ => panic!("runtime handle kind mismatch: expected NaN-boxed value"),
        });
        runtime_write_barrier_root_nanbox(bits);
    }

    pub fn get_heap_word_u64(&self) -> u64 {
        self.with_slot(|slot| match slot {
            RuntimeHandleSlot::HeapWord(bits) => bits,
            _ => panic!("runtime handle kind mismatch: expected heap word"),
        })
    }

    pub fn set_heap_word_u64(&self, bits: u64) {
        self.with_slot_mut(|slot| match slot {
            RuntimeHandleSlot::HeapWord(current) => *current = bits,
            _ => panic!("runtime handle kind mismatch: expected heap word"),
        });
        runtime_write_barrier_root_heap_word(bits);
    }

    pub fn get_raw_mut_ptr<T>(&self) -> *mut T {
        self.with_slot(|slot| match slot {
            RuntimeHandleSlot::RawTagged { addr, .. } => addr as *mut T,
            _ => panic!("runtime handle kind mismatch: expected raw pointer"),
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

    pub fn get_raw_const_ptr<T>(&self) -> *const T {
        self.with_slot(|slot| match slot {
            RuntimeHandleSlot::RawTagged { addr, .. } => addr as *const T,
            _ => panic!("runtime handle kind mismatch: expected raw pointer"),
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
