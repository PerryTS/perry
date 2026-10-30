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
///   check, which is the limit glibc's maps lookup reports as well;
/// * when the rlimit is not a size the mapping can have (`RLIM_INFINITY`, or
///   one reaching the address-space bottom) the kernel lays the address space
///   out bottom-up and the stack may grow into whatever is free, down to a
///   neighbouring mapping that can be terabytes away. glibc and musl then
///   report that gap as the stack size, which puts the overflow guard so far
///   down that the process exhausts memory or hits a mapping first (SIGSEGV).
///   The bound is then the kernel's own default stack limit,
///   [`UNLIMITED_MAIN_STACK`], the size a process has when nobody set one.
///
/// The low bound this yields equals glibc's on every finite rlimit and
/// environment size tried (the top differs from glibc's by at most one page).
/// The derivation is checked against the one fact the process knows
/// independently (a live address of this very stack lies below the mapping end
/// by less than the stack size); anything that fails the check, or a missing
/// `AT_EXECFN`, keeps asking pthread, exactly as before.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn stack_bounds() -> Option<(usize, usize)> {
    #[cfg(target_os = "linux")]
    if let Some(bounds) = main_thread_bounds() {
        return Some(bounds);
    }
    pthread_bounds()
}

/// The kernel's default `RLIMIT_STACK` (`_STK_LIM`, 8 MiB): the main-thread
/// stack size when the rlimit gives no usable bound.
#[cfg(target_os = "linux")]
const UNLIMITED_MAIN_STACK: usize = 8 << 20;

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
/// `None` on any other thread or when the derived mapping end fails its
/// consistency check.
#[cfg(target_os = "linux")]
fn main_thread_bounds() -> Option<(usize, usize)> {
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
    derive_main_bounds(rl.rlim_cur, end, live_stack_address())
}

/// An address inside the calling (main) thread's stack, for the consistency
/// check: glibc records the stack pointer at process entry (set by the loader;
/// also present in a static glibc, and no lookup is needed to read it).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn live_stack_address() -> usize {
    extern "C" {
        static __libc_stack_end: *mut libc::c_void;
    }
    // SAFETY: written once by the loader before any user code runs.
    unsafe { __libc_stack_end as usize }
}

/// An address inside the calling thread's stack (libcs without
/// `__libc_stack_end`).
#[cfg(all(target_os = "linux", not(target_env = "gnu")))]
fn live_stack_address() -> usize {
    let here = 0u8;
    std::hint::black_box(&here) as *const u8 as usize
}

/// The main thread's `(low, size)` from the rlimit, the mapping end and a live
/// address `here` of the stack; `None` when they disagree.
#[cfg(target_os = "linux")]
fn derive_main_bounds(rlim_cur: libc::rlim_t, end: usize, here: usize) -> Option<(usize, usize)> {
    // RLIM_INFINITY is above every mapping end, so one comparison covers it.
    let size = match usize::try_from(rlim_cur) {
        Ok(size) if size < end => size,
        _ => UNLIMITED_MAIN_STACK,
    };
    // The live address lies inside the mapping, strictly above its low end
    // (which also rules out a zero size) and below its end.
    let low = end.checked_sub(size)?;
    if here <= low || here >= end {
        return None;
    }
    Some((low, size))
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
    use super::{derive_main_bounds, stack_top, UNLIMITED_MAIN_STACK};

    const END: usize = 0x7ffc_0000_0000;

    #[test]
    fn a_finite_rlimit_is_the_stack_size() {
        let size = 64 << 20;
        assert_eq!(
            derive_main_bounds(size as libc::rlim_t, END, END - 4096),
            Some((END - size, size))
        );
    }

    /// `ulimit -s unlimited` (and a limit reaching the address-space bottom)
    /// must still give a finite bound the overflow guard can sit above.
    #[test]
    fn an_unusable_rlimit_gets_the_kernel_default_stack() {
        for rlim in [libc::RLIM_INFINITY, END as libc::rlim_t, !0 >> 1] {
            assert_eq!(
                derive_main_bounds(rlim, END, END - 4096),
                Some((END - UNLIMITED_MAIN_STACK, UNLIMITED_MAIN_STACK)),
                "rlimit {rlim:#x}"
            );
        }
    }

    #[test]
    fn a_live_address_outside_the_derived_stack_is_refused() {
        let far = END - UNLIMITED_MAIN_STACK - 4096;
        assert_eq!(derive_main_bounds(libc::RLIM_INFINITY, END, far), None);
        assert_eq!(derive_main_bounds(libc::RLIM_INFINITY, END, END), None);
    }

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
