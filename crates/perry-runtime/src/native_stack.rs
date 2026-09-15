//! Linux stack bounds shared by the collector and error-frame walkers.

/// Return the exclusive upper bound of this thread's native stack, or zero
/// when pthread cannot supply it. Callers use zero to abandon the stack walk.
pub(crate) fn stack_top() -> usize {
    // libc supplies the target's exact size, alignment and declarations.
    // Hand-written byte buffers and externs previously disagreed across the
    // three consumers, causing clashing_extern_declarations in Linux CI.
    let mut attr = std::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
    let mut addr: *mut libc::c_void = std::ptr::null_mut();
    let mut size: usize = 0;
    // SAFETY: pthread_getattr_np initializes attr on success. Only then may
    // getstack read it and destroy release its resources. Both output slots
    // are live, correctly typed locals; the stack address is never dereferenced.
    let ok = unsafe {
        if libc::pthread_getattr_np(libc::pthread_self(), attr.as_mut_ptr()) != 0 {
            return 0;
        }
        let ok = libc::pthread_attr_getstack(attr.as_ptr(), &mut addr, &mut size) == 0;
        libc::pthread_attr_destroy(attr.as_mut_ptr());
        ok
    };
    if !ok || addr.is_null() || size == 0 {
        return 0;
    }
    (addr as usize).checked_add(size).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::stack_top;

    #[test]
    fn stack_top_encloses_a_current_thread_local() {
        let local = 0u8;
        let address = std::hint::black_box(&local) as *const u8 as usize;
        assert!(
            stack_top() > address,
            "pthread must bound the current stack"
        );
    }

    /// The bound must be the CALLING thread's, which is what a stack walk
    /// started on a worker depends on: reading the main thread's bound there
    /// would run the walk off the end of a much larger region.
    ///
    /// The discriminating fact is that a worker's bound differs from the main
    /// thread's, so that is what this asserts. It deliberately does NOT
    /// compare `top - address` against the REQUESTED stack size: how much the
    /// allocator rounds the request up to, and whether the guard page counts
    /// inside the reported region, are per-platform (and, on a hosted CI
    /// runner, per-image) properties, and that comparison failed on Linux CI
    /// while holding on this project's own Linux box and on macOS. A test that
    /// green-lights one runner's rounding is not testing the bound.
    #[test]
    fn stack_top_respects_custom_thread_stack_sizes() {
        let main_top = stack_top();
        for stack_size in [256 * 1024, 2 * 1024 * 1024] {
            std::thread::Builder::new()
                .stack_size(stack_size)
                .spawn(move || {
                    let local = 0u8;
                    let address = std::hint::black_box(&local) as *const u8 as usize;
                    let top = stack_top();
                    assert!(top > address, "worker stack bound must enclose its local");
                    assert!(
                        main_top == 0 || top != main_top,
                        "bound must belong to this worker, not the spawning thread \
                         (worker top {top:#x}, spawning thread top {main_top:#x})"
                    );
                })
                .unwrap()
                .join()
                .unwrap();
        }
    }
}
