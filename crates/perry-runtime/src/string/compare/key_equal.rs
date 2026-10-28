//! Property-key byte equality with reads bounded by the actual slices.
//!
//! libc's vector comparison chooses a slower path when a short image string
//! happens to begin near a page boundary. Compare machine words within the
//! payload instead, so moving unrelated image data does not change that path.

#[inline]
pub(crate) fn key_bytes_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    if a.as_ptr() == b.as_ptr() {
        return true;
    }
    let n = a.len();
    // The length proves every unaligned load stays inside each operand.
    // Overlapping the last word avoids a byte loop without reading padding.
    unsafe {
        if n >= 8 {
            let mut offset = 0;
            while offset + 8 < n {
                if a.as_ptr().add(offset).cast::<u64>().read_unaligned()
                    != b.as_ptr().add(offset).cast::<u64>().read_unaligned()
                {
                    return false;
                }
                offset += 8;
            }
            return a.as_ptr().add(n - 8).cast::<u64>().read_unaligned()
                == b.as_ptr().add(n - 8).cast::<u64>().read_unaligned();
        }
        if n >= 4 {
            return a.as_ptr().cast::<u32>().read_unaligned()
                == b.as_ptr().cast::<u32>().read_unaligned()
                && a.as_ptr().add(n - 4).cast::<u32>().read_unaligned()
                    == b.as_ptr().add(n - 4).cast::<u32>().read_unaligned();
        }
        if n >= 2 {
            return a.as_ptr().cast::<u16>().read_unaligned()
                == b.as_ptr().cast::<u16>().read_unaligned()
                && a[n - 1] == b[n - 1];
        }
        n == 0 || a[0] == b[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_covers_lengths_alignments_and_every_byte() {
        for n in 0..=129 {
            for alignment in 0..32 {
                let a: Vec<_> = (0..n).map(|i| (i * 173) as u8).collect();
                let mut storage = vec![0; alignment + n];
                let b = &mut storage[alignment..];
                b.copy_from_slice(&a);
                assert!(key_bytes_equal(&a, b));
                assert!(!key_bytes_equal(&a, &[&b[..], &[0u8][..]].concat()));
                for i in 0..n {
                    b[i] ^= 1;
                    assert!(
                        !key_bytes_equal(&a, b),
                        "len={n} alignment={alignment} byte={i}"
                    );
                    b[i] ^= 1;
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn equality_never_reads_past_a_guard_page() {
        unsafe {
            let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
            let allocation = libc::mmap(
                std::ptr::null_mut(),
                page * 2,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            );
            assert_ne!(allocation, libc::MAP_FAILED);
            let boundary = (allocation as *mut u8).add(page);
            assert_eq!(libc::mprotect(boundary.cast(), page, libc::PROT_NONE), 0);
            for n in 0..=129 {
                let expected: Vec<_> = (0..n).map(|i| (i * 173) as u8).collect();
                let start = boundary.sub(n);
                std::ptr::copy_nonoverlapping(expected.as_ptr(), start, n);
                assert!(key_bytes_equal(
                    &expected,
                    std::slice::from_raw_parts(start, n)
                ));
                assert!(key_bytes_equal(
                    std::slice::from_raw_parts(start, n),
                    &expected
                ));
                for i in 0..n {
                    *start.add(i) ^= 1;
                    assert!(!key_bytes_equal(
                        &expected,
                        std::slice::from_raw_parts(start, n)
                    ));
                    assert!(!key_bytes_equal(
                        std::slice::from_raw_parts(start, n),
                        &expected
                    ));
                    *start.add(i) ^= 1;
                }
            }
            assert_eq!(libc::munmap(allocation, page * 2), 0);
        }
    }
}
