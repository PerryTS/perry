//! #6882/#7450: mimalloc's OS mappings must carry VM tag 240, not the default
//! 100 — macOS tooling decodes tag 100 as `IOAccelerator`, so the whole JS
//! heap shows up as GPU-driver memory in vmmap/Instruments/footprint.
//!
//! The primary assertion here is on the *mapping*, not on
//! `mi_option_get(mi_option_os_tag)`. #6882 shipped with only the
//! option-value assertion, and it passed for the entire time the feature was
//! broken: the option really was 240, it had just been set after mimalloc
//! already reserved — and thereby tagged — the arena backing the whole heap.
//! Reading the option back tests that a store landed in mimalloc's static
//! table. Reading the kernel's `user_tag` for a live heap address tests what
//! anyone following the profiling recipe actually sees.

#![cfg(all(
    target_pointer_width = "64",
    target_vendor = "apple",
    feature = "alloc-mimalloc"
))]

use crate::mimalloc_os_tag::PERRY_HEAP_VM_TAG;

const VM_REGION_EXTENDED_INFO: libc::c_int = 13;

/// `struct vm_region_extended_info` from `<mach/vm_region.h>`. `user_tag` is
/// the field `vmmap` renders as the region type.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VmRegionExtendedInfo {
    protection: libc::c_int,
    user_tag: libc::c_uint,
    pages_resident: libc::c_uint,
    pages_shared_now_private: libc::c_uint,
    pages_swapped_out: libc::c_uint,
    pages_dirtied: libc::c_uint,
    ref_count: libc::c_uint,
    shadow_depth: libc::c_ushort,
    external_pager: libc::c_uchar,
    share_mode: libc::c_uchar,
    pages_reusable: libc::c_uint,
}

extern "C" {
    /// The calling task's port. `libc::mach_task_self()` wraps this but is
    /// deprecated in favour of the `mach2` crate, which the runtime does not
    /// depend on; the underlying global is stable ABI.
    static mach_task_self_: libc::mach_port_t;

    fn mach_vm_region(
        target_task: libc::mach_port_t,
        address: *mut u64,
        size: *mut u64,
        flavor: libc::c_int,
        info: *mut libc::c_int,
        info_count: *mut libc::c_uint,
        object_name: *mut libc::mach_port_t,
    ) -> libc::kern_return_t;
}

/// The kernel's VM tag for the mapping containing `addr`, or `None` if the
/// lookup failed or walked past `addr` into a later region.
fn mapping_vm_tag(addr: usize) -> Option<u32> {
    let mut region_start = addr as u64;
    let mut region_size: u64 = 0;
    let mut info = VmRegionExtendedInfo::default();
    let mut info_count =
        (std::mem::size_of::<VmRegionExtendedInfo>() / std::mem::size_of::<libc::c_uint>()) as u32;
    let mut object_name: libc::mach_port_t = 0;
    // SAFETY: self-inspection of the calling task. `mach_vm_region` writes at
    // most `info_count` words into `info`, and `info_count` is derived from
    // `size_of::<VmRegionExtendedInfo>()`.
    let kr = unsafe {
        mach_vm_region(
            mach_task_self_,
            &mut region_start,
            &mut region_size,
            VM_REGION_EXTENDED_INFO,
            (&raw mut info).cast::<libc::c_int>(),
            &mut info_count,
            &mut object_name,
        )
    };
    if kr != 0 {
        return None;
    }
    // `mach_vm_region` returns the first region at or above the address it is
    // handed; if it skipped forward, `addr` itself is unmapped.
    let contains = region_start <= addr as u64 && (addr as u64) < region_start + region_size;
    contains.then_some(info.user_tag)
}

#[test]
fn mimalloc_heap_mappings_carry_the_perry_vm_tag() {
    // The runtime defers to an operator steering the tag by hand, so the
    // assertion only holds when nothing is.
    if std::env::var_os("MIMALLOC_OS_TAG").is_some() {
        return;
    }
    crate::gc::js_gc_init();

    // A multi-megabyte allocation through the global allocator — i.e. through
    // mimalloc, i.e. backed by one of the OS mappings under test. Touched at
    // both ends so the pages are real rather than a lazy reservation.
    let mut heap_block = vec![0u8; 8 << 20];
    let last = heap_block.len() - 1;
    heap_block[0] = 1;
    heap_block[last] = 1;

    let tag = mapping_vm_tag(heap_block.as_ptr() as usize)
        .expect("a live mimalloc allocation must sit inside a mapped VM region");
    assert_eq!(
        libc::c_long::from(tag),
        PERRY_HEAP_VM_TAG,
        "mimalloc's heap mappings must carry VM tag {PERRY_HEAP_VM_TAG} \
         (VM_MEMORY_APPLICATION_SPECIFIC_1); got {tag}. Tag 100 renders the \
         heap as IOAccelerator in vmmap/Instruments (#6882), and is what you \
         get whenever the retag runs after mimalloc has already reserved its \
         arena — i.e. from anywhere other than a pre-`main` module initializer \
         (#7450). Check that the `__DATA,__mod_init_func` entry in \
         `mimalloc_os_tag` survived the link."
    );
}

#[test]
fn js_gc_init_leaves_the_os_tag_option_set() {
    // Strictly weaker than the mapping assertion above, and kept only as a
    // localizer: if both fail, the option store itself is broken; if only the
    // mapping test fails, the constructor did not run early enough (or at all).
    if std::env::var_os("MIMALLOC_OS_TAG").is_some() {
        return;
    }
    crate::gc::js_gc_init();
    let tag = unsafe { libmimalloc_sys::mi_option_get(libmimalloc_sys::mi_option_os_tag) };
    assert_eq!(tag, PERRY_HEAP_VM_TAG);
}
