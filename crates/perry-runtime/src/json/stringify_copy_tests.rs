use super::*;

#[test]
fn json_piece_copy_preserves_bytes_and_sentinels_at_every_short_alignment() {
    for len in 0..=64 {
        for source_offset in 0..16 {
            let source: Vec<u8> = (0..len + 32).map(|i| (i * 173 + 19) as u8).collect();
            for output_offset in 0..16 {
                let mut output = vec![0x5a; len + 48];
                let start = 16 + output_offset;
                unsafe {
                    copy_bytes(
                        source.as_ptr().add(source_offset),
                        output.as_mut_ptr().add(start),
                        len,
                    );
                }
                assert_eq!(
                    &output[start..start + len],
                    &source[source_offset..source_offset + len]
                );
                assert!(output[..start].iter().all(|&b| b == 0x5a));
                assert!(output[start + len..].iter().all(|&b| b == 0x5a));
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn json_piece_copy_neither_reads_nor_writes_past_guard_pages() {
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let allocate = || {
            let p = libc::mmap(
                std::ptr::null_mut(),
                page * 3,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            );
            assert_ne!(p, libc::MAP_FAILED);
            assert_eq!(
                libc::mprotect(
                    p.cast::<u8>().add(page).cast(),
                    page,
                    libc::PROT_READ | libc::PROT_WRITE
                ),
                0
            );
            p.cast::<u8>()
        };
        let source = allocate();
        let output = allocate();
        for len in 0..=64 {
            for source_at_end in [false, true] {
                for output_at_end in [false, true] {
                    let from = source.add(if source_at_end { 2 * page - len } else { page });
                    let to = output.add(if output_at_end { 2 * page - len } else { page });
                    for i in 0..len {
                        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                        from.add(i).write((i * 137 + 11) as u8);
                    }
                    copy_bytes(from, to, len);
                    for i in 0..len {
                        assert_eq!(to.add(i).read(), (i * 137 + 11) as u8);
                    }
                }
            }
        }
        assert_eq!(libc::munmap(source.cast(), page * 3), 0);
        assert_eq!(libc::munmap(output.cast(), page * 3), 0);
    }
}
