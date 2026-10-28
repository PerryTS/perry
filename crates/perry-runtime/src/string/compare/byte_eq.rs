//! Exact byte equality using reads bounded by each slice's length.
//!
//! Short property names need no out-of-line memcmp or alignment dispatch.
//! Two overlapping words cover a slice up to twice their width; neither read
//! crosses the slice boundary. Longer strings keep the standard comparison.
#[inline(always)]
pub(super) fn equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let n = a.len();
    // Compare both covering words together: equality needs no branch on the
    // first word, and every load stays within its operand, even at a page end.
    unsafe {
        if n >= 8 {
            if n > 16 {
                return a == b;
            }
            let front = a.as_ptr().cast::<u64>().read_unaligned()
                ^ b.as_ptr().cast::<u64>().read_unaligned();
            let tail = a.as_ptr().add(n - 8).cast::<u64>().read_unaligned()
                ^ b.as_ptr().add(n - 8).cast::<u64>().read_unaligned();
            (front | tail) == 0
        } else if n >= 4 {
            let front = a.as_ptr().cast::<u32>().read_unaligned()
                ^ b.as_ptr().cast::<u32>().read_unaligned();
            let tail = a.as_ptr().add(n - 4).cast::<u32>().read_unaligned()
                ^ b.as_ptr().add(n - 4).cast::<u32>().read_unaligned();
            (front | tail) == 0
        } else if n >= 2 {
            let front = a.as_ptr().cast::<u16>().read_unaligned()
                ^ b.as_ptr().cast::<u16>().read_unaligned();
            let tail = a.as_ptr().add(n - 2).cast::<u16>().read_unaligned()
                ^ b.as_ptr().add(n - 2).cast::<u16>().read_unaligned();
            (front | tail) == 0
        } else {
            n == 0 || *a.as_ptr() == *b.as_ptr()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::equal;

    #[test]
    fn pointer_admission_excludes_the_entire_sentinel_range() {
        use crate::string::{is_valid_string_ptr, StringHeader};
        for address in 0..0x1000 {
            assert!(!is_valid_string_ptr(address as *const StringHeader));
        }
        for address in [0x1000, 0x1001, 0xffff, usize::MAX] {
            assert!(is_valid_string_ptr(address as *const StringHeader));
        }
    }

    #[test]
    fn exact_bytes_at_all_word_boundaries_and_alignments() {
        let source: Vec<u8> = (0..96).map(|i| (i * 71) as u8).collect();
        for n in 0..=40 {
            for offset in 0..16 {
                let a = &source[offset..offset + n];
                let mut b = a.to_vec();
                assert!(equal(a, &b));
                for i in 0..n {
                    b[i] ^= 1;
                    assert!(!equal(a, &b), "n={n} offset={offset} byte={i}");
                    b[i] ^= 1;
                }
                b.push(0);
                assert!(!equal(a, &b));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn each_operand_stops_before_a_guard_page() {
        unsafe {
            let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
            let mapping = libc::mmap(
                std::ptr::null_mut(),
                page * 4,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            );
            assert_ne!(mapping, libc::MAP_FAILED);
            let a_end = (mapping as *mut u8).add(page);
            let b_end = (mapping as *mut u8).add(page * 3);
            assert_eq!(libc::mprotect(a_end.cast(), page, libc::PROT_NONE), 0);
            assert_eq!(libc::mprotect(b_end.cast(), page, libc::PROT_NONE), 0);
            for n in 0..=40 {
                let a = a_end.sub(n);
                let b = b_end.sub(n);
                for i in 0..n {
                    *a.add(i) = (i * 71) as u8;
                    *b.add(i) = *a.add(i);
                }
                assert!(equal(
                    std::slice::from_raw_parts(a, n),
                    std::slice::from_raw_parts(b, n)
                ));
                for i in 0..n {
                    *b.add(i) ^= 1;
                    assert!(!equal(
                        std::slice::from_raw_parts(a, n),
                        std::slice::from_raw_parts(b, n)
                    ));
                    *b.add(i) ^= 1;
                }
            }
            assert_eq!(libc::munmap(mapping, page * 4), 0);
        }
    }
}
