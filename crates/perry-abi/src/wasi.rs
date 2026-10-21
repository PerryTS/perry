//! Preview 1 core ABI signatures, shared by the runtime and optional host.
//! The syscall implementation lives in wasi-common; this list only marshals
//! scalar arguments across Perry's archive boundary.
#[macro_export]
macro_rules! wasi_preview1_functions {
    ($m:ident) => { $m! {
        fn args_get(argv: i32, argv_buf: i32) -> i32;
        fn args_sizes_get(offset0: i32, offset1: i32) -> i32;
        fn clock_res_get(id: i32, offset0: i32) -> i32;
        fn clock_time_get(id: i32, precision: i64, offset0: i32) -> i32;
        fn environ_get(environ: i32, environ_buf: i32) -> i32;
        fn environ_sizes_get(offset0: i32, offset1: i32) -> i32;
        fn fd_advise(fd: i32, offset: i64, len: i64, advice: i32) -> i32;
        fn fd_allocate(fd: i32, offset: i64, len: i64) -> i32;
        fn fd_close(fd: i32) -> i32;
        fn fd_datasync(fd: i32) -> i32;
        fn fd_fdstat_get(fd: i32, offset0: i32) -> i32;
        fn fd_fdstat_set_flags(fd: i32, flags: i32) -> i32;
        fn fd_fdstat_set_rights(fd: i32, fs_rights_base: i64, fs_rights_inheriting: i64) -> i32;
        fn fd_filestat_get(fd: i32, offset0: i32) -> i32;
        fn fd_filestat_set_size(fd: i32, size: i64) -> i32;
        fn fd_filestat_set_times(fd: i32, atim: i64, mtim: i64, fst_flags: i32) -> i32;
        fn fd_pread(fd: i32, iov_buf: i32, iov_buf_len: i32, offset: i64, offset0: i32) -> i32;
        fn fd_prestat_dir_name(fd: i32, path: i32, path_len: i32) -> i32;
        fn fd_prestat_get(fd: i32, offset0: i32) -> i32;
        fn fd_pwrite(fd: i32, ciov_buf: i32, ciov_buf_len: i32, offset: i64, offset0: i32) -> i32;
        fn fd_read(fd: i32, iov_buf: i32, iov_buf_len: i32, offset1: i32) -> i32;
        fn fd_readdir(fd: i32, buf: i32, buf_len: i32, cookie: i64, offset0: i32) -> i32;
        fn fd_renumber(fd: i32, to: i32) -> i32;
        fn fd_seek(fd: i32, offset: i64, whence: i32, offset0: i32) -> i32;
        fn fd_sync(fd: i32) -> i32;
        fn fd_tell(fd: i32, offset0: i32) -> i32;
        fn fd_write(fd: i32, ciov_buf: i32, ciov_buf_len: i32, offset0: i32) -> i32;
        fn path_create_directory(fd: i32, offset: i32, length: i32) -> i32;
        fn path_filestat_get(fd: i32, flags: i32, offset: i32, length: i32, offset0: i32) -> i32;
        fn path_filestat_set_times(fd: i32, flags: i32, offset: i32, length: i32, atim: i64, mtim: i64, fst_flags: i32) -> i32;
        fn path_link(old_fd: i32, old_flags: i32, old_offset: i32, old_length: i32, new_fd: i32, new_offset: i32, new_length: i32) -> i32;
        fn path_open(fd: i32, dirflags: i32, offset: i32, length: i32, oflags: i32, fs_rights_base: i64, fs_rights_inheriting: i64, fdflags: i32, offset0: i32) -> i32;
        fn path_readlink(fd: i32, offset: i32, length: i32, buf: i32, buf_len: i32, offset0: i32) -> i32;
        fn path_remove_directory(fd: i32, offset: i32, length: i32) -> i32;
        fn path_rename(fd: i32, old_offset: i32, old_length: i32, new_fd: i32, new_offset: i32, new_length: i32) -> i32;
        fn path_symlink(old_offset: i32, old_length: i32, fd: i32, new_offset: i32, new_length: i32) -> i32;
        fn path_unlink_file(fd: i32, offset: i32, length: i32) -> i32;
        fn poll_oneoff(in_: i32, out: i32, nsubscriptions: i32, offset0: i32) -> i32;
        fn proc_exit(rval: i32) -> ();
        fn proc_raise(sig: i32) -> i32;
        fn random_get(buf: i32, buf_len: i32) -> i32;
        fn sched_yield() -> i32;
        fn sock_accept(fd: i32, flags: i32, offset0: i32) -> i32;
        fn sock_recv(fd: i32, iov_buf: i32, iov_buf_len: i32, ri_flags: i32, offset0: i32, offset1: i32) -> i32;
        fn sock_send(fd: i32, ciov_buf: i32, ciov_buf_len: i32, si_flags: i32, offset0: i32) -> i32;
        fn sock_shutdown(fd: i32, how: i32) -> i32;
    }};
}
macro_rules! kind {
    (i32) => {
        0u8
    };
    (i64) => {
        1u8
    };
}
macro_rules! metadata {
    ($(fn $name:ident($($arg:ident: $ty:ident),* $(,)?) -> $ret:tt;)*) => {
        pub const NAMES: &[&str] = &[$(stringify!($name)),*];
        pub const PARAMS: &[&[u8]] = &[$(&[$(kind!($ty)),*]),*];
    }
}
wasi_preview1_functions!(metadata);
/// Import callback outcomes. Exit and trap must unwind the interpreter.
pub const IMPORT_EXIT: i32 = -1;
pub const IMPORT_TRAP: i32 = -2;
