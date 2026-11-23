use super::*;
use windows_sys::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_FREE, MEM_RESERVE,
};

/// Committed bytes of `[data, data + len)`, by the kernel's page states.
pub(in crate::arena) fn committed_bytes(data: *mut u8, len: usize) -> usize {
    let mut at = data as usize;
    let end = at + len;
    let mut committed = 0;
    while at < end {
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let got = unsafe {
            VirtualQuery(
                at as *const _,
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        assert_ne!(got, 0, "VirtualQuery failed");
        let run_end = (info.BaseAddress as usize + info.RegionSize).min(end);
        if info.State == MEM_COMMIT {
            committed += run_end - at;
        }
        at = run_end;
    }
    committed
}

fn state_at(data: *mut u8) -> u32 {
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let got = unsafe {
        VirtualQuery(
            data as *const _,
            &mut info,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    assert_ne!(got, 0, "VirtualQuery failed");
    info.State
}

#[test]
fn a_released_region_returns_its_commit_and_reuse_commits_it_again() {
    unsafe {
        for kind in [Kind::NurseryBlock, Kind::OldBlock, Kind::LargeObject] {
            let len = 2 * super::super::BLOCK_SIZE;
            let data = map(kind, len);
            assert!(!data.is_null());
            assert_eq!(data as usize % super::super::GENERATION_PAGE_SIZE, 0);
            assert_eq!(
                committed_bytes(data, len),
                len,
                "fresh backing is committed"
            );
            std::ptr::write_bytes(data, 0xa5, len);
            assert!(contains(data as usize, len));

            assert_eq!(release(data, len), len);
            assert_eq!(
                committed_bytes(data, len),
                0,
                "LIVE SUBJECT: commit returned"
            );
            assert_eq!(state_at(data), MEM_RESERVE, "the reservation stays");
            assert!(
                !contains(data as usize, 16),
                "a decommitted extent holds no object a caller may read"
            );
            // Releasing again is harmless and keeps the range released.
            assert_eq!(release(data, len), len);

            reuse(data, len);
            assert_eq!(committed_bytes(data, len), len);
            assert!(contains(data as usize, len));
            // Recommitted pages are demand-zero, writable and readable.
            assert_eq!(*data, 0);
            assert_eq!(*data.add(len - 1), 0);
            std::ptr::write_bytes(data, 0x5a, len);
            assert_eq!(*data.add(len / 2), 0x5a);

            // Reuse of a committed region keeps its contents.
            reuse(data, len);
            assert_eq!(*data.add(len - 1), 0x5a);

            unmap(data, len);
            assert!(!contains(data as usize, 16));
            assert_eq!(state_at(data), MEM_FREE, "unmap releases the reservation");
        }
    }
}

#[test]
fn a_released_region_can_be_unmapped_without_reuse() {
    unsafe {
        let len = super::super::BLOCK_SIZE;
        let data = map(Kind::NurseryBlock, len);
        assert!(!data.is_null());
        assert_eq!(release(data, len), len);
        unmap(data, len);
        assert_eq!(state_at(data), MEM_FREE);
    }
}

#[test]
fn extents_round_up_to_whole_os_pages() {
    assert_eq!(windows::extent(0), None);
    assert_eq!(windows::extent(1), Some(4096));
    assert_eq!(windows::extent(4096), Some(4096));
    assert_eq!(windows::extent(4097), Some(8192));
    assert_eq!(windows::extent(usize::MAX), None);
}

/// The smallest block a caller may map is one metadata page (`map` asserts
/// whole `GENERATION_PAGE_SIZE` pages); its whole extent is released, reused
/// and unmapped.
#[test]
fn a_one_page_block_releases_and_reuses_its_whole_extent() {
    let len = super::super::GENERATION_PAGE_SIZE;
    unsafe {
        let data = map(Kind::NurseryBlock, len);
        assert!(!data.is_null());
        assert_eq!(backing_len(len), len);
        assert!(contains(data as usize, len));
        assert_eq!(release(data, len), len);
        reuse(data, len);
        unmap(data, len);
    }
}
