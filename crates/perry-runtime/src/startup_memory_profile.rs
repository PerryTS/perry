//! Opt-in policy for small Linux processes. Configure the allocator before
//! Rust startup can allocate; setting this from `js_gc_init` is too late.
//!
//! `mi_option_set_default` leaves mimalloc's own environment parsing intact,
//! including its lowercase option spelling. No Rust allocation, environment
//! mutation, or lock belongs on this constructor path.

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
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn allocator_policy_is_selected_before_gc_init() {
            // The runner launches this test in a fresh process for each
            // profile/override. Rust's test harness has already allocated;
            // js_gc_init is deliberately never called here.
            let profile = std::env::var("PERRY_MEMORY_PROFILE").ok();
            let explicit = std::env::var("MIMALLOC_ALLOW_THP")
                .or_else(|_| std::env::var("mimalloc_allow_thp"))
                .ok();
            let expected = match explicit.as_deref() {
                Some("0") => 0,
                Some("1") => 1,
                None => i64::from(profile.as_deref() != Some("small")),
                value => panic!("probe expects an absent, 0, or 1 override: {value:?}"),
            };
            let actual = unsafe { super::perry_memory_profile_allow_thp() };
            assert_eq!(actual as i64, expected);
            // On Linux this is the process-wide effect of allow_thp=0. The
            // option value alone would pass even if the constructor ran too
            // late for mimalloc's OS initialization to apply the policy.
            let disabled = unsafe { libc::prctl(libc::PR_GET_THP_DISABLE, 0, 0, 0, 0) };
            assert!(disabled >= 0, "PR_GET_THP_DISABLE must be available");
            if expected == 0 {
                assert_eq!(
                    disabled, 1,
                    "THP policy must already be applied before gc_init"
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
