//! Native mimalloc THP is disabled before Rust startup. Linux advised-only
//! process mode permits Perry's region advice on supporting kernels; older
//! kernels safely retain full THP disable. The constructor never allocates.

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "64",
    feature = "alloc-mimalloc"
))]
mod linux {
    extern "C" {
        fn perry_retain_memory_profile_init();
        #[cfg(test)]
        fn perry_memory_profile_allow_thp() -> libc::c_long;
    }

    #[inline(never)]
    pub(super) fn retain_constructor() {
        // Pull the C constructor's archive member into native programs even
        // with multiple codegen units. This function does not apply a policy
        // late; its constructor must already have run before Rust startup.
        unsafe { perry_retain_memory_profile_init() };
        if crate::gc::gc_diag_enabled() {
            let mode = unsafe { libc::prctl(libc::PR_GET_THP_DISABLE, 0, 0, 0, 0) };
            let name = match mode {
                3 => "except-advised",
                1 => "base-pages",
                _ => "unexpected",
            };
            eprintln!("[gc-region-thp] mode={name} prctl={mode}");
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn allocator_policy_is_selected_before_gc_init() {
            // The runner launches this test in a fresh process for each
            // profile/override. Rust's test harness has already allocated;
            // js_gc_init is deliberately never called here.
            let expected = 0;
            let actual = unsafe { super::perry_memory_profile_allow_thp() };
            assert_eq!(actual as i64, expected);
            // On Linux this is the process-wide effect of allow_thp=0. The
            // option value alone would pass even if the constructor ran too
            // late for mimalloc's OS initialization to apply the policy.
            let disabled = unsafe { libc::prctl(libc::PR_GET_THP_DISABLE, 0, 0, 0, 0) };
            assert!(disabled >= 0, "PR_GET_THP_DISABLE must be available");
            if expected == 0 {
                assert!(
                    disabled == 3 || disabled == 1,
                    "THP policy must be advised-only or the old-kernel disabled fallback: {disabled}"
                );
            }
        }
    }
}

/// Keep the pre-main constructor in a statically linked Perry executable.
#[inline(never)]
pub(crate) fn retain_constructor() {
    #[cfg(all(
        target_os = "linux",
        target_pointer_width = "64",
        feature = "alloc-mimalloc"
    ))]
    linux::retain_constructor();
}
