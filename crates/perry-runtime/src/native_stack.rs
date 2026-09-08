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

    #[test]
    fn stack_top_is_specific_to_concurrent_custom_stack_workers() {
        // `Builder::stack_size` is a request, not the exact size Linux must
        // report for the resulting pthread mapping. In particular, the glibc
        // stack cache may satisfy a request with a larger reusable mapping.
        // Keep both workers alive together and assert the actual invariant:
        // each bound encloses its worker and names neither the parent nor its
        // concurrently live peer.
        let parent_top = stack_top();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let workers: Vec<_> = [256 * 1024, 2 * 1024 * 1024]
            .into_iter()
            .map(|stack_size| {
                let barrier = barrier.clone();
                std::thread::Builder::new()
                    .stack_size(stack_size)
                    .spawn(move || {
                        let local = 0u8;
                        let address = std::hint::black_box(&local) as *const u8 as usize;
                        let top = stack_top();
                        barrier.wait();
                        (address, top)
                    })
                    .unwrap()
            })
            .collect();
        barrier.wait();
        let bounds: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();

        for (address, top) in &bounds {
            assert!(top > address, "worker stack bound must enclose its local");
            assert_ne!(*top, parent_top, "worker must not reuse the parent bound");
        }
        assert_ne!(
            bounds[0].1, bounds[1].1,
            "live workers need distinct bounds"
        );
    }
}
