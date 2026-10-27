use super::*;

struct ShortReader<'a> {
    bytes: &'a [u8],
    limit: usize,
    interrupt: bool,
    first_buffer: *const u8,
}

impl Read for ShortReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.interrupt = true;
        if self.first_buffer.is_null() && out.len() > 1 {
            self.first_buffer = out.as_ptr();
        }
        let n = out.len().min(self.bytes.len()).min(self.limit);
        out[..n].copy_from_slice(&self.bytes[..n]);
        self.bytes = &self.bytes[n..];
        Ok(n)
    }
}

fn check(bytes: &[u8], hint: u64, limit: usize) {
    let mut reader = ShortReader {
        bytes,
        limit,
        interrupt: true,
        first_buffer: ptr::null(),
    };
    let actual = read_utf8(&mut reader, hint).unwrap();
    let expected = String::from_utf8_lossy(bytes);
    unsafe {
        assert_eq!(
            slice::from_raw_parts(string_data(actual), (*actual).byte_len as usize),
            expected.as_bytes()
        );
        assert_eq!(
            (*actual).utf16_len as usize,
            expected.encode_utf16().count()
        );
        assert_eq!(
            (*actual).flags & STRING_FLAG_WTF8_VALIDATED,
            STRING_FLAG_WTF8_VALIDATED
        );
        assert_eq!((*actual).flags & STRING_FLAG_HAS_LONE_SURROGATES, 0);
        if hint as usize >= bytes.len() && !bytes.is_empty() && str::from_utf8(bytes).is_ok() {
            // The reader wrote into the returned string, with no final copy.
            if hint > 1 {
                assert_eq!(reader.first_buffer, string_data(actual));
            }
            assert_eq!((*actual).capacity as u64, hint);
        }
    }
}

#[test]
fn short_reads_eintr_hints_and_split_utf8_match_lossy_decoding() {
    let _global = crate::gc::global_side_table_test_lock();
    let samples: &[&[u8]] = &[
        b"",
        b"ascii\0data",
        "\u{feff}é中🙂tail".as_bytes(),
        &[0xed, 0xa0, 0x80, 0xed, 0xb0, 0x80],
        &[0xf0, 0x9f, 0x99],
        &[b'a', 0xc0, 0xaf, 0xff, b'b'],
        &[0xe2, 0x82, b'X', 0x80, 0xe2, 0x82],
    ];
    for bytes in samples {
        for hint in [0, 1, bytes.len() as u64, bytes.len() as u64 + 11] {
            for limit in 1..=7 {
                check(bytes, hint, limit);
            }
        }
    }
}

#[test]
fn read_chunk_boundaries_and_final_storage_are_preserved() {
    let _global = crate::gc::global_side_table_test_lock();
    for split in 0..4 {
        let mut bytes = vec![b'a'; READ_CHUNK - split];
        bytes.extend_from_slice("🙂é".as_bytes());
        bytes.extend_from_slice(&vec![b'z'; READ_CHUNK + 1]);
        check(&bytes, bytes.len() as u64, READ_CHUNK);
        check(&bytes, 7, READ_CHUNK);
    }
}

#[test]
fn hard_read_failure_is_not_published_as_a_partial_string() {
    let _global = crate::gc::global_side_table_test_lock();
    struct Fails(bool);
    impl Read for Fails {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.0 {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            self.0 = true;
            out[0] = b'x';
            Ok(1)
        }
    }
    assert_eq!(
        read_utf8(&mut Fails(false), 8).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn native_file_output_has_gc_owned_lifetime() {
    let _global = crate::gc::global_side_table_test_lock();
    let path = std::env::temp_dir().join(format!("JFS-native-string-{}", std::process::id()));
    let bytes = "\u{feff}é🙂".repeat(20000);
    std::fs::write(&path, bytes.as_bytes()).unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let string = {
        let mut file = std::fs::File::open(&path).unwrap();
        read_utf8(&mut file, bytes.len() as u64).unwrap()
    };
    std::fs::remove_file(&path).unwrap();
    let root = scope.root_string_ptr(string);
    crate::gc::js_gc_collect();
    unsafe {
        root.with_string_bytes(|actual| assert_eq!(actual, bytes.as_bytes()));
    }
}
