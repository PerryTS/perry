//! Fixed-size roots for native callback loops, using the runtime handle scanner.
use super::{RuntimeHandle, RuntimeHandleScope};

pub(crate) fn with_stack_roots<const N: usize, R>(
    values: [*mut u8; N],
    f: impl FnOnce(&StackRoots<'_, N>) -> R,
) -> R {
    let scope = RuntimeHandleScope::new();
    let roots = StackRoots {
        handles: values.map(|word| scope.root_raw_mut_ptr(word)),
    };
    f(&roots)
}

pub(crate) struct StackRoots<'a, const N: usize> {
    handles: [RuntimeHandle<'a>; N],
}

impl<const N: usize> StackRoots<'_, N> {
    #[inline(always)]
    pub(crate) fn with_const_ptr<T, R>(&self, index: usize, f: impl FnOnce(*const T) -> R) -> R {
        self.handles[index].with_const_ptr(f)
    }
}
