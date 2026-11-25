//! A descriptor opened on an embedded asset serves the registry's bytes
//! through the same fd operations as a file on disk.
use super::*;

const NAME: &[u8] = b"embedded-fd/asset.txt";
const BYTES: &[u8] = b"0123456789";
const PATH: &str = "$perryfs/embedded-fd/asset.txt";

fn boxed(text: &str) -> f64 {
    crate::value::js_nanbox_string(js_string_from_bytes(text.as_ptr(), text.len() as u32) as i64)
}

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn open(path: &str, flags: f64) -> Result<i32, std::io::Error> {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| unsafe {
        crate::embedded::js_register_embedded_asset(
            NAME.as_ptr(),
            NAME.len(),
            BYTES.as_ptr(),
            BYTES.len(),
        );
    });
    unsafe { fs_open_sync_result(boxed(path), flags) }.map_err(|(error, _)| error)
}

fn read(fd: i32, length: usize, position: f64) -> Vec<u8> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let buffer = scope.root_nanbox_f64(crate::buffer::bytes::from_slice(
        crate::buffer::bytes::Brand::Buffer,
        &vec![0; length],
    ));
    let read = read_sync_result(
        fd as f64,
        buffer.get_nanbox_f64(),
        0.0,
        length as f64,
        position,
    )
    .unwrap();
    bytes_from_value(buffer.get_nanbox_f64())[..read as usize].to_vec()
}

#[test]
fn embedded_fd_reads_sequentially_and_by_position() {
    let _global = crate::gc::global_side_table_test_lock();
    let fd = open(PATH, undefined()).expect("embedded asset opens");
    assert_eq!(read(fd, 4, undefined()), b"0123");
    // A positional read leaves the descriptor's own offset alone.
    assert_eq!(read(fd, 2, 8.0), b"89");
    assert_eq!(read(fd, 16, undefined()), b"456789");
    assert_eq!(read(fd, 4, undefined()), b"");
    assert_eq!(js_fs_close_sync(fd as f64), 1);
    assert!(!fd_is_registered(fd));
}

#[test]
fn embedded_fd_read_file_returns_the_rest_and_advances() {
    let _global = crate::gc::global_side_table_test_lock();
    let fd = open(PATH, boxed("r")).expect("embedded asset opens");
    assert_eq!(read(fd, 3, undefined()), b"012");
    let rest = |fd: i32| {
        read_file_bytes_with_options(fd as f64, undefined())
            .ok()
            .unwrap()
    };
    assert_eq!(rest(fd), b"3456789");
    assert_eq!(rest(fd), b"");
    js_fs_close_sync(fd as f64);
}

#[test]
fn embedded_fd_stats_as_a_read_only_file() {
    let _global = crate::gc::global_side_table_test_lock();
    let fd = open(PATH, 0.0).expect("embedded asset opens with O_RDONLY");
    let stats = fstat_stats_value(fd, false).ok().unwrap();
    assert_eq!(
        unsafe { options_number_field(stats, b"size") },
        Some(BYTES.len() as f64)
    );
    let write = write_string_sync_result(fd, boxed("x"), undefined()).unwrap_err();
    assert_eq!(write.raw_os_error(), ebadf_os_error().raw_os_error());
    assert!(unsafe { fd_sync_ops::js_fs_ftruncate_result(fd as f64, 0.0) }.is_err());
    assert_eq!(js_fs_fsync_sync(fd as f64), 1);
    assert!(try_clone_registered_fd(fd).is_none());
    assert!(fd_is_embedded(fd));
    js_fs_close_sync(fd as f64);
}

#[test]
fn embedded_open_requires_a_registered_asset_and_a_read_flag() {
    let _global = crate::gc::global_side_table_test_lock();
    let missing = open("$perryfs/embedded-fd/absent.txt", undefined()).unwrap_err();
    assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
    for flags in ["r+", "w", "a"] {
        assert!(open(PATH, boxed(flags)).is_err(), "{flags} opened an asset");
    }
    #[cfg(unix)]
    for flags in [libc::O_CREAT, libc::O_APPEND, libc::O_TRUNC, libc::O_WRONLY] {
        assert!(open(PATH, flags as f64).is_err(), "{flags} opened an asset");
    }
}
