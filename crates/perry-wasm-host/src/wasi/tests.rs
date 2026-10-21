use super::*;
fn context(preopens: Vec<(String, String)>) -> WasiCtx {
    build(
        Config {
            args: vec!["guest".into(), "hello λ".into()],
            env: vec!["ONLY=guest".into()],
            preopens,
        },
        &[-1; 3],
    )
    .unwrap()
}
fn call(ctx: &mut WasiCtx, mem: &mut [u8], name: &str, args: &[u64]) -> i32 {
    invoke(ctx, mem, name, args).unwrap()
}
fn u32_at(mem: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(mem[at..at + 4].try_into().unwrap())
}
fn put32(mem: &mut [u8], at: usize, value: u32) {
    mem[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn open(ctx: &mut WasiCtx, mem: &mut [u8], path: &str) -> Result<u32, i32> {
    mem[128..128 + path.len()].copy_from_slice(path.as_bytes());
    let errno = call(
        ctx,
        mem,
        "path_open",
        &[3, 1, 128, path.len() as u64, 0, 2, 0, 0, 64],
    );
    if errno == 0 {
        Ok(u32_at(mem, 64))
    } else {
        Err(errno)
    }
}
fn read(ctx: &mut WasiCtx, mem: &mut [u8], fd: u32) -> String {
    put32(mem, 0, 512);
    put32(mem, 4, 128);
    assert_eq!(call(ctx, mem, "fd_read", &[fd as u64, 0, 1, 8]), 0);
    String::from_utf8(mem[512..512 + u32_at(mem, 8) as usize].to_vec()).unwrap()
}
#[test]
fn args_environment_clocks_random_and_bounds() {
    let mut ctx = context(vec![]);
    let mut mem = vec![0; 4096];
    assert_eq!(call(&mut ctx, &mut mem, "args_sizes_get", &[0, 4]), 0);
    assert_eq!(u32_at(&mem, 0), 2);
    assert_eq!(call(&mut ctx, &mut mem, "args_get", &[8, 32]), 0);
    assert_eq!(&mem[32..47], b"guest\0hello \xce\xbb\0");
    assert_eq!(call(&mut ctx, &mut mem, "environ_sizes_get", &[0, 4]), 0);
    assert_eq!(u32_at(&mem, 0), 1);
    assert_eq!(call(&mut ctx, &mut mem, "environ_get", &[8, 32]), 0);
    assert_eq!(&mem[32..43], b"ONLY=guest\0");
    assert_eq!(call(&mut ctx, &mut mem, "clock_time_get", &[0, 0, 0]), 0);
    assert_ne!(&mem[..8], &[0; 8]);
    assert_eq!(call(&mut ctx, &mut mem, "clock_time_get", &[1, 0, 8]), 0);
    assert_eq!(call(&mut ctx, &mut mem, "clock_res_get", &[999, 0]), 28);
    assert_eq!(call(&mut ctx, &mut mem, "random_get", &[128, 128]), 0);
    let first = mem[128..256].to_vec();
    assert_eq!(call(&mut ctx, &mut mem, "random_get", &[128, 128]), 0);
    assert_ne!(first, mem[128..256]);
    // wiggle validates pointers; an out-of-bounds buffer cannot touch host memory.
    assert!(
        invoke(&mut ctx, &mut mem, "random_get", &[4090, 128]).is_err()
            || call(&mut ctx, &mut mem, "random_get", &[4090, 128]) != 0
    );
}
#[test]
fn preopens_read_files_and_contexts_have_separate_descriptors() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    std::fs::write(a.path().join("data"), "alpha").unwrap();
    std::fs::write(b.path().join("data"), "beta").unwrap();
    let mut a = context(vec![("/sandbox".into(), a.path().display().to_string())]);
    let mut b = context(vec![("/sandbox".into(), b.path().display().to_string())]);
    let mut mem = vec![0; 4096];
    assert_eq!(call(&mut a, &mut mem, "fd_prestat_get", &[3, 0]), 0);
    assert_eq!(u32_at(&mem, 4), 8);
    let afd = open(&mut a, &mut mem, "data").unwrap();
    let bfd = open(&mut b, &mut mem, "data").unwrap();
    assert_eq!(afd, bfd);
    assert_eq!(read(&mut a, &mut mem, afd), "alpha");
    assert_eq!(call(&mut a, &mut mem, "fd_close", &[afd as u64]), 0);
    assert_eq!(read(&mut b, &mut mem, bfd), "beta");
    assert_ne!(call(&mut a, &mut mem, "fd_fdstat_get", &[afd as u64, 0]), 0);
    let mut empty = context(vec![]);
    assert_ne!(call(&mut empty, &mut mem, "fd_prestat_get", &[3, 0]), 0);
}
#[test]
fn preopen_rejects_parent_absolute_and_symlink_escape() {
    let root = tempfile::tempdir().unwrap();
    let sandbox = root.path().join("sandbox");
    std::fs::create_dir(&sandbox).unwrap();
    let outside = root.path().join("secret");
    std::fs::write(&outside, "secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, sandbox.join("escape")).unwrap();
    let mut ctx = context(vec![("/sandbox".into(), sandbox.display().to_string())]);
    let mut mem = vec![0; 4096];
    assert!(open(&mut ctx, &mut mem, "../secret").is_err());
    assert!(open(&mut ctx, &mut mem, &outside.display().to_string()).is_err());
    #[cfg(unix)]
    assert!(open(&mut ctx, &mut mem, "escape").is_err());
}
#[test]
fn directory_operations_and_poll_clock() {
    let root = tempfile::tempdir().unwrap();
    let mut ctx = context(vec![("/sandbox".into(), root.path().display().to_string())]);
    let mut mem = vec![0; 4096];
    mem[128..131].copy_from_slice(b"dir");
    assert_eq!(
        call(&mut ctx, &mut mem, "path_create_directory", &[3, 128, 3]),
        0
    );
    assert!(root.path().join("dir").is_dir());
    assert_eq!(
        call(&mut ctx, &mut mem, "path_remove_directory", &[3, 128, 3]),
        0
    );
    assert!(!root.path().join("dir").exists());
    mem.fill(0);
    mem[0..8].copy_from_slice(&42u64.to_le_bytes());
    put32(&mut mem, 16, 1); // monotonic clock
    mem[24..32].copy_from_slice(&1u64.to_le_bytes()); // relative timeout
    assert_eq!(
        call(&mut ctx, &mut mem, "poll_oneoff", &[0, 128, 1, 256]),
        0
    );
    assert_eq!(u32_at(&mem, 256), 1);
    assert_eq!(u32_at(&mem, 128), 42);
    assert_eq!(u32_at(&mem, 136), 0); // event errno/type zero (clock)
}
#[cfg(unix)]
#[test]
fn custom_stdio_is_duplicated_and_guest_close_preserves_embedder() {
    use std::io::{Seek, Write};
    use std::os::fd::AsRawFd;
    let mut output = tempfile::tempfile().unwrap();
    let mut ctx = build(
        Config {
            args: vec![],
            env: vec![],
            preopens: vec![],
        },
        &[-1, output.as_raw_fd() as i64, -1],
    )
    .unwrap();
    let mut mem = vec![0; 128];
    mem[32..37].copy_from_slice(b"hello");
    put32(&mut mem, 0, 32);
    put32(&mut mem, 4, 5);
    assert_eq!(call(&mut ctx, &mut mem, "fd_write", &[1, 0, 1, 8]), 0);
    assert_eq!(u32_at(&mem, 8), 5);
    assert_eq!(call(&mut ctx, &mut mem, "fd_close", &[1]), 0);
    drop(ctx);
    output.write_all(b"!").unwrap();
    output.rewind().unwrap();
    let mut value = String::new();
    std::io::Read::read_to_string(&mut output, &mut value).unwrap();
    assert_eq!(value, "hello!");
}
#[test]
fn exit_is_a_structured_error_and_full_u32_status_survives() {
    let mut ctx = context(vec![]);
    let mut mem = vec![0; 8];
    let error = invoke(&mut ctx, &mut mem, "proc_exit", &[0x80000000]).unwrap_err();
    assert_eq!(
        error.downcast::<wasi_common::I32Exit>().unwrap().0,
        i32::MIN
    );
    let error = invoke(&mut ctx, &mut mem, "proc_exit", &[7]).unwrap_err();
    assert_eq!(error.downcast::<wasi_common::I32Exit>().unwrap().0, 7);
}

#[cfg(target_os = "linux")]
#[test]
fn dropping_context_releases_preopened_directory() {
    let root = tempfile::tempdir().unwrap();
    let count = || {
        std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| std::fs::read_link(entry.path()).ok().as_deref() == Some(root.path()))
            .count()
    };
    let before = count();
    let ctx = context(vec![("/sandbox".into(), root.path().display().to_string())]);
    assert!(count() > before, "test must observe the live capability");
    drop(ctx);
    assert_eq!(count(), before);
}
