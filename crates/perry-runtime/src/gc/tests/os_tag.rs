//! #6882: `js_gc_init` must move mimalloc's OS-mapping VM tag off 100 —
//! macOS tooling decodes tag 100 as `IOAccelerator`, so the whole JS heap
//! shows up as GPU-driver memory in vmmap/Instruments/footprint.

#[cfg(all(target_pointer_width = "64", target_vendor = "apple"))]
#[test]
fn js_gc_init_retags_mimalloc_os_mappings() {
    // Only asserts when the profiler override isn't steering the tag —
    // js_gc_init deliberately defers to an explicit MIMALLOC_OS_TAG.
    if std::env::var_os("MIMALLOC_OS_TAG").is_some() {
        return;
    }
    crate::gc::js_gc_init();
    let tag = unsafe { libmimalloc_sys::mi_option_get(libmimalloc_sys::mi_option_os_tag) };
    assert_eq!(
        tag, 240,
        "js_gc_init must retag mimalloc mappings to VM_MEMORY_APPLICATION_SPECIFIC_1 \
         (240); tag 100 renders the heap as IOAccelerator in macOS tools (#6882)"
    );
}
