//! Test the runtime's native page mapper. Production is in hot_text.c so an
//! unordered link cannot perturb Rust code generation, ThinLTO names or data.
#[cfg(target_os = "linux")]
const HP: usize = 2 * 1024 * 1024;

#[cfg(target_os = "linux")]
extern "C" {
    fn js_back_hot_text(start: *const u8, limit: *const u8);
}

#[cfg(target_os = "linux")]
unsafe fn back(start: usize, limit: usize) {
    js_back_hot_text(start as *const u8, limit as *const u8);
}

#[cfg(target_os = "linux")]
fn thp_enabled(policy: &str) -> bool {
    policy.contains("[always]") || policy.contains("[madvise]")
}

#[cfg(target_os = "linux")]
mod tests {
    use super::*;

    // Isolate process-wide THP policy and executable mappings from the
    // runtime test process. This verifies the live operation, including
    // its OFF state, and checks every byte across the mapping boundary.
    #[test]
    fn bounded_remap_preserves_bytes_and_process_disable_is_a_noop() {
        unsafe {
            let pid = libc::fork();
            assert!(pid >= 0);
            if pid == 0 {
                let result = std::panic::catch_unwind(|| check_mapping());
                let code = match result {
                    Ok(()) => 0,
                    Err(error) => {
                        if let Some(message) = error.downcast_ref::<String>() {
                            libc::write(2, message.as_ptr().cast(), message.len());
                        } else if let Some(message) = error.downcast_ref::<&str>() {
                            libc::write(2, message.as_ptr().cast(), message.len());
                        }
                        1
                    }
                };
                libc::_exit(code);
            }
            let mut status = 0;
            assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
            assert_eq!(status, 0, "isolated remap check failed");
        }
    }

    unsafe fn check_mapping() {
        let reserve = libc::mmap(
            std::ptr::null_mut(),
            HP * 3,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_ne!(reserve, libc::MAP_FAILED);
        let base = reserve as usize;
        let start = (base + HP - 1) & !(HP - 1);
        let end = start + HP * 2;
        if start > base {
            libc::munmap(reserve, start - base);
        }
        if end < base + HP * 3 {
            libc::munmap(end as *mut libc::c_void, base + HP * 3 - end);
        }
        let ptr = start as *mut u8;
        for i in 0..HP * 2 {
            // GC_STORE_AUDIT(POINTER_FREE): Both writes fill owned mmap test spans with (i%251) as u8 before remap/mprotect checks. Byte patterns, not GC child fields.
            ptr.add(i).write((i % 251) as u8);
        }
        // Prevent in-place anonymous collapse on the fixture. The ON
        // arm must exercise the prepared-copy remap, leaving the second
        // page with NOHUGEPAGE while the first gets HUGEPAGE.
        assert_eq!(libc::madvise(ptr.cast(), HP * 2, libc::MADV_NOHUGEPAGE), 0);
        assert_eq!(
            libc::mprotect(ptr.cast(), HP * 2, libc::PROT_READ | libc::PROT_EXEC),
            0
        );
        assert_eq!(libc::prctl(libc::PR_SET_THP_DISABLE, 1, 0, 0, 0), 0);
        back(start, start + HP);
        let off = std::fs::read_to_string("/proc/self/maps").unwrap();
        // An untouched mapping still spans both halves. A remap would
        // split it at the hot-prefix boundary even if bytes were copied.
        assert!(
            off.lines()
                .any(|line| line.starts_with(&format!("{start:x}-{end:x} "))),
            "disabled THP remapped text: {off}"
        );
        assert_eq!(
            libc::prctl(
                libc::PR_SET_THP_DISABLE,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong
            ),
            0
        );
        // The runtime disables automatic THP while allowing explicit advice
        // on supporting kernels. Exercise that actual startup policy too.
        if libc::prctl(
            libc::PR_SET_THP_DISABLE,
            1 as libc::c_ulong,
            2 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        ) == 0
        {
            assert_eq!(
                libc::prctl(
                    libc::PR_GET_THP_DISABLE,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong
                ),
                3
            );
        }
        back(start, start + HP);
        for i in 0..HP * 2 {
            assert_eq!(ptr.add(i).read(), (i % 251) as u8, "byte {i}");
        }
        // The second huge page must retain the original mapping. It has
        // no huge-page advice; only the selected first page may get it.
        let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap();
        let mut beyond = false;
        let mut prefix = false;
        let mut remapped = false;
        for line in smaps.lines() {
            if line.contains('-') && line.contains(" r") {
                if let Some((range, _)) = line.split_once(' ') {
                    let (lo, hi) = range.split_once('-').unwrap();
                    let lo = usize::from_str_radix(lo, 16).unwrap();
                    let hi = usize::from_str_radix(hi, 16).unwrap();
                    beyond = lo <= start + HP && start + HP < hi;
                    prefix = lo == start && hi == start + HP;
                }
            } else if line.starts_with("VmFlags:") {
                let hg = line.split_whitespace().any(|f| f == "hg");
                if beyond {
                    assert!(!hg, "remapped past selected prefix");
                }
                if prefix {
                    remapped |= hg;
                }
            }
        }
        let policy = std::fs::read_to_string("/sys/kernel/mm/transparent_hugepage/enabled")
            .unwrap_or_default();
        if thp_enabled(&policy) {
            assert!(remapped, "ON arm did not exercise the remap");
        }
        assert_eq!(libc::munmap(ptr.cast(), HP * 2), 0);
    }

    #[test]
    fn partial_boundary_pages_stay_file_backed() {
        unsafe {
            let pid = libc::fork();
            assert!(pid >= 0);
            if pid == 0 {
                let result = std::panic::catch_unwind(|| check_partial_pages());
                libc::_exit(if result.is_ok() { 0 } else { 1 });
            }
            let mut status = 0;
            assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
            assert_eq!(status, 0, "partial-page backing escaped selected bounds");
        }
    }

    unsafe fn check_partial_pages() {
        let reserve = libc::mmap(
            std::ptr::null_mut(),
            HP * 4,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_ne!(reserve, libc::MAP_FAILED);
        let base = reserve as usize;
        let start = (base + HP - 1) & !(HP - 1);
        let end = start + HP * 3;
        if start > base {
            libc::munmap(reserve, start - base);
        }
        if end < base + HP * 4 {
            libc::munmap(end as *mut libc::c_void, base + HP * 4 - end);
        }
        let ptr = start as *mut u8;
        for i in 0..HP * 3 {
            // GC_STORE_AUDIT(POINTER_FREE): Both writes fill owned mmap test spans with (i%251) as u8 before remap/mprotect checks. Byte patterns, not GC child fields.
            ptr.add(i).write((i % 251) as u8);
        }
        assert_eq!(libc::madvise(ptr.cast(), HP * 3, libc::MADV_NOHUGEPAGE), 0);
        assert_eq!(
            libc::mprotect(ptr.cast(), HP * 3, libc::PROT_READ | libc::PROT_EXEC),
            0
        );
        assert_eq!(libc::prctl(libc::PR_SET_THP_DISABLE, 0, 0, 0, 0), 0);
        // Exactly the middle page fits: copying either edge includes code
        // outside the selected interval and must be detected below.
        back(start + 16, end - 16);
        for i in 0..HP * 3 {
            assert_eq!(ptr.add(i).read(), (i % 251) as u8);
        }
        let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap();
        let mut range = (0, 0);
        let mut backed = false;
        for line in smaps.lines() {
            if let Some(first) = line.split_whitespace().next() {
                if let Some((lo, hi)) = first.split_once('-') {
                    if let (Ok(lo), Ok(hi)) =
                        (usize::from_str_radix(lo, 16), usize::from_str_radix(hi, 16))
                    {
                        range = (lo, hi);
                    }
                }
            }
            if line.starts_with("VmFlags:") && line.split_whitespace().any(|f| f == "hg") {
                if range.0 < end && range.1 > start {
                    assert_eq!(
                        range,
                        (start + HP, start + HP * 2),
                        "backed a partial boundary page"
                    );
                    backed = true;
                }
            }
        }
        let policy = std::fs::read_to_string("/sys/kernel/mm/transparent_hugepage/enabled")
            .unwrap_or_default();
        if thp_enabled(&policy) {
            assert!(backed, "middle complete page was not backed");
        }
        assert_eq!(libc::munmap(ptr.cast(), HP * 3), 0);
    }

    #[test]
    fn empty_and_unaligned_bounds_are_noops() {
        unsafe {
            back(0, HP);
            back(HP, HP);
            back(HP + 1, HP * 2);
            back(HP, HP * 2 - 1);
        }
    }
}
