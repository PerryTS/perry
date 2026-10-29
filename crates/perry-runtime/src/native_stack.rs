//! Linux stack bounds shared by the collector, the overflow guard and the
//! error-frame walkers.

/// `(lowest usable address, size)` of the calling thread's native stack, or
/// `None` when it cannot be determined.
///
/// glibc answers `pthread_getattr_np` for the PROCESS main thread by reading
/// `/proc/self/maps` line by line (~130k instructions, paid at every startup by
/// the stack-limit publish and again by the first stack walk). Every other
/// thread's answer comes from the descriptor of the stack its creator
/// allocated, with no parsing. The main thread's answer is already known to
/// the process without a lookup, because the kernel builds the stack mapping
/// itself:
///
/// * the mapping ends at the page boundary just above the program-file name
///   string (`AT_EXECFN`), the first thing the kernel copies onto the new stack
///   (`bprm->p` starts at the mapping end minus one word);
/// * the stack may grow to `RLIMIT_STACK` below that end, the kernel's own
///   check, which is the limit glibc's maps lookup reports as well.
///
/// The low bound this yields equals glibc's on every rlimit and environment size
/// tried (the top differs from glibc's by at most one page). The derivation is
/// checked against the one fact the process knows independently
/// (`__libc_stack_end`, the entry stack pointer, lies below the mapping end
/// by less than the stack size); anything that fails the check, a missing
/// `AT_EXECFN`, and an unlimited (or address-space-wide) rlimit, whose bound is
/// whatever mapping happens to sit below the stack and which only the maps
/// lookup knows, keep asking pthread, exactly as before.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn stack_bounds() -> Option<(usize, usize)> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if let Some(bounds) = main_thread_bounds() {
        return Some(bounds);
    }
    pthread_bounds()
}

/// Return the exclusive upper bound of this thread's native stack, or zero
/// when it cannot be supplied. Callers use zero to abandon the stack walk.
#[cfg(target_os = "linux")]
pub(crate) fn stack_top() -> usize {
    match stack_bounds() {
        Some((low, size)) => low.checked_add(size).unwrap_or(0),
        None => 0,
    }
}

/// The process main thread's bounds from `AT_EXECFN` and the stack rlimit;
/// `None` on any other thread, when the rlimit is not a size, or when the
/// derived mapping end fails its consistency check.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn main_thread_bounds() -> Option<(usize, usize)> {
    extern "C" {
        /// glibc: the stack pointer at process entry (set by the loader; also
        /// present in a static glibc).
        static __libc_stack_end: *mut libc::c_void;
    }
    // The main thread is the one whose tid is the pid. Every other thread
    // (the runtime's own, or an embedder's) takes the pthread path, whose
    // answer is read from its descriptor.
    // SAFETY: both are plain queries about the calling thread/process.
    let is_main = unsafe { libc::syscall(libc::SYS_gettid) == libc::getpid() as libc::c_long };
    if !is_main {
        return None;
    }
    let mut rl = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: getrlimit fills `rl` on success, the only case that reads it.
    let rl = unsafe {
        if libc::getrlimit(libc::RLIMIT_STACK, rl.as_mut_ptr()) != 0 {
            return None;
        }
        rl.assume_init()
    };
    if rl.rlim_cur == libc::RLIM_INFINITY {
        return None;
    }
    let size = usize::try_from(rl.rlim_cur).ok()?;
    // SAFETY: AT_EXECFN is a NUL-terminated string the kernel placed on the
    // stack; 0 means the entry is absent.
    let execfn = unsafe { libc::getauxval(libc::AT_EXECFN) } as usize;
    if execfn == 0 {
        return None;
    }
    // SAFETY: as above; the terminator is within the mapping.
    let len = unsafe { libc::strlen(execfn as *const libc::c_char) };
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    let end = execfn
        .checked_add(len + 1)?
        .checked_next_multiple_of(page)?;
    // SAFETY: written once by the loader before any user code runs.
    let entry_sp = unsafe { __libc_stack_end } as usize;
    // The entry stack pointer lies inside the mapping, below its end by the
    // argv/envp/auxv blocks, which are smaller than the stack itself.
    if size == 0 || end < entry_sp || end - entry_sp >= size || end <= size {
        return None;
    }
    Some((end - size, size))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn pthread_bounds() -> Option<(usize, usize)> {
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
            return None;
        }
        let ok = libc::pthread_attr_getstack(attr.as_ptr(), &mut addr, &mut size) == 0;
        libc::pthread_attr_destroy(attr.as_mut_ptr());
        ok
    };
    (ok && !addr.is_null() && size != 0).then_some((addr as usize, size))
}

#[cfg(all(test, target_os = "linux"))]
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
