use super::*;

fn boxed(text: &str) -> f64 {
    crate::value::js_nanbox_string(js_string_from_bytes(text.as_ptr(), text.len() as u32) as i64)
}

#[test]
fn embedded_and_missing_virtual_files_follow_the_same_text_decoder() {
    let _global = crate::gc::global_side_table_test_lock();
    const NAME: &[u8] = b"JFS/read-file-input.bin";
    const BYTES: &[u8] = &[0xef, 0xbb, 0xbf, 0xf0, 0x9f, 0x99, 0x82, 0xff, 0];
    unsafe {
        crate::embedded::js_register_embedded_asset(
            NAME.as_ptr(),
            NAME.len(),
            BYTES.as_ptr(),
            BYTES.len(),
        );
    }
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let s =
        read_file_string_with_options(boxed("$perryfs/JFS/read-file-input.bin"), undefined, "utf8")
            .ok()
            .unwrap();
    assert_eq!(
        crate::string::string_as_str(s),
        String::from_utf8_lossy(BYTES)
    );
    assert_eq!(
        read_file_bytes_with_options(boxed("$perryfs/JFS/read-file-input.bin"), undefined)
            .ok()
            .unwrap(),
        BYTES
    );
    let error = read_file_string_with_options(boxed("$perryfs/JFS/absent"), undefined, "utf8")
        .err()
        .unwrap();
    let _ = error; // Node-parity coverage checks code/syscall/path.
}

#[cfg(unix)]
#[test]
fn pipe_fd_reads_until_eof_and_keeps_the_descriptor_open() {
    use std::os::fd::FromRawFd;
    let _global = crate::gc::global_side_table_test_lock();
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let input = unsafe { fs::File::from_raw_fd(fds[0]) };
    let mut output = unsafe { fs::File::from_raw_fd(fds[1]) };
    let bytes = "é🙂pipe".repeat(30000);
    let expected = bytes.clone();
    let writer = std::thread::spawn(move || output.write_all(bytes.as_bytes()).unwrap());
    let fd = allocate_synthetic_fd();
    FD_REGISTRY.with(|r| r.borrow_mut().insert(fd, input));
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let s = read_file_string_with_options(fd as f64, undefined, "utf8")
        .ok()
        .unwrap();
    writer.join().unwrap();
    assert_eq!(crate::string::string_as_str(s), expected);
    assert!(fd_is_registered(fd));
    FD_REGISTRY.with(|r| r.borrow_mut().remove(&fd));
}

#[test]
fn utf16le_lone_surrogates_are_preserved_and_pairs_are_canonical() {
    let _global = crate::gc::global_side_table_test_lock();
    let bytes = [0, 0xd8, 0x41, 0, 0x3d, 0xd8, 0x42, 0xde, 0, 0xdc, 0xff];
    let s = decode_bytes(&bytes, "utf16le");
    assert_eq!(crate::string::js_string_length(s), 5);
    for (i, unit) in [0xd800, 0x41, 0xd83d, 0xde42, 0xdc00].iter().enumerate() {
        assert_eq!(
            crate::string::js_string_char_code_at(s, i as i32),
            *unit as f64
        );
    }
    unsafe {
        assert_ne!(
            (*s).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES,
            0
        );
        assert_ne!((*s).flags & crate::string::STRING_FLAG_WTF8_VALIDATED, 0);
    }
}
