use super::*;
use mach2::kern_return::{KERN_INVALID_ADDRESS, KERN_SUCCESS};
use mach2::vm_region::{vm_region_extended_info, VM_REGION_EXTENDED_INFO};

unsafe fn region_for(data: *mut u8) -> Option<(u64, u64, vm_region_extended_info)> {
    let mut address = data as u64;
    let mut size = 0;
    let mut info = vm_region_extended_info::default();
    let mut count = vm_region_extended_info::count();
    let mut object = 0;
    let result = mach2::vm::mach_vm_region(
        mach2::traps::mach_task_self(),
        &mut address,
        &mut size,
        VM_REGION_EXTENDED_INFO,
        (&mut info as *mut vm_region_extended_info).cast(),
        &mut count,
        &mut object,
    );
    if result == KERN_INVALID_ADDRESS {
        return None;
    }
    assert_eq!(result, KERN_SUCCESS);
    if object != 0 {
        assert_eq!(
            mach2::mach_port::mach_port_deallocate(mach2::traps::mach_task_self(), object),
            0
        );
    }
    Some((address, size, info))
}

/// The kernel's view of the VM region holding `data`.
pub(crate) unsafe fn info_for(data: *mut u8) -> vm_region_extended_info {
    let (address, size, info) = region_for(data).expect("region missing");
    assert!(address <= data as u64 && (data as u64) < address + size);
    info
}

#[test]
fn apple_blocks_are_one_mebibyte_regions() {
    assert_eq!(apple::ALIGN, 1024 * 1024);
    assert_eq!(crate::arena::BLOCK_SIZE, apple::ALIGN);
    assert_eq!(backing_len(apple::ALIGN), apple::ALIGN, "no rounding slack");
    assert_eq!(backing_len(apple::ALIGN + 16 * 1024), 2 * apple::ALIGN);
}

#[test]
fn apple_regions_are_aligned_tagged_and_registered() {
    unsafe {
        let page = 16 * 1024;
        for kind in [Kind::NurseryBlock, Kind::OldBlock, Kind::LargeObject] {
            for len in [
                page,
                apple::ALIGN - page,
                apple::ALIGN,
                apple::ALIGN + page,
                2 * apple::ALIGN,
            ] {
                let data = map(kind, len);
                assert!(!data.is_null());
                assert_eq!(data as usize % apple::ALIGN, 0);
                let tag = info_for(data).user_tag;
                assert_eq!(tag, apple::VM_TAG);
                assert_ne!(tag, 240, "GC regions must have their own VM category");
                assert!(contains(data as usize, backing_len(len)));
                std::ptr::write_bytes(data, 0xa5, len);
                unmap(data, len);
                assert!(!contains(data as usize, 1));
                if let Some((address, _, _)) = region_for(data) {
                    assert!(address > data as u64, "unmap must remove the region");
                }
            }
        }
    }
}

#[test]
fn apple_release_and_reuse_round_trip_changes_kernel_reusable_state() {
    unsafe {
        let len = 8 * apple::ALIGN;
        let data = map(Kind::OldBlock, len);
        assert!(!data.is_null());
        std::ptr::write_bytes(data, 0xa5, len);
        assert!(
            info_for(data).pages_resident > 0,
            "live subject: touched pages"
        );
        assert_eq!(release(data, len), len);
        let reusable = info_for(data).pages_reusable;
        assert!(reusable > 0, "live subject: kernel marked pages reusable");
        reuse(data, len);
        let reusable = info_for(data).pages_reusable;
        assert_eq!(reusable, 0, "REUSE must reactivate pages before a write");
        std::ptr::write_bytes(data, 0x5a, len);
        assert_eq!(*data, 0x5a);
        assert_eq!(*data.add(len - 1), 0x5a);
        unmap(data, len);
    }
}

#[test]
fn apple_invalid_extents_are_refused() {
    unsafe {
        assert!(map(Kind::NurseryBlock, 0).is_null());
        assert!(map(Kind::NurseryBlock, usize::MAX & !(apple::ALIGN - 1)).is_null());
    }
}
