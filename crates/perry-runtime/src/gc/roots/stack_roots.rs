//! Fixed-size roots for native callback loops, using the runtime handle scanner.
use super::{RuntimeHandle, RuntimeHandleScope};

pub(crate) fn with_stack_roots<const N: usize, R>(
    values: [u64; N],
    f: impl FnOnce(&StackRoots<'_, N>) -> R,
) -> R {
    let scope = RuntimeHandleScope::new();
    let roots = StackRoots {
        handles: values.map(|word| scope.root_heap_word_u64(word)),
    };
    f(&roots)
}

pub(crate) struct StackRoots<'a, const N: usize> {
    handles: [RuntimeHandle<'a>; N],
}

impl<const N: usize> StackRoots<'_, N> {
    #[inline(always)]
    pub(crate) fn get(&self, index: usize) -> u64 {
        self.handles[index].get_heap_word_u64()
    }
}
