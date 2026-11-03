//! Unix filesystem system calls, executed only on blocking-pool threads.
use super::{Object, output};
use crate::{
    Error, ErrorKind, Result,
    fs::{
        AccessMode, FileMetadata, FileOptions, FileTime, FileType, FsOutput, FsPath, FsRequest,
        FsTarget, TimeChange, put_record,
    },
};
use std::{
    ffi::{CStr, c_int},
    os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd},
    ptr::NonNull,
};

pub(in crate::fs) struct File(OwnedFd);
pub(in crate::fs) struct Dir(NonNull<libc::DIR>);
// SAFETY: the directory stream is used by one pool thread at a time, under the
// handle's mutex, and closed exactly once by Drop.
unsafe impl Send for Dir {}
impl Drop for Dir {
    fn drop(&mut self) {
        // SAFETY: this owner holds the only reference to the open stream.
        unsafe { libc::closedir(self.0.as_ptr()) };
    }
}

fn last() -> Error {
    std::io::Error::last_os_error().into()
}
fn os(code: c_int) -> Error {
    std::io::Error::from_raw_os_error(code).into()
}
fn check(result: c_int) -> Result<()> {
    if result < 0 { Err(last()) } else { Ok(()) }
}
/// Retry a call interrupted before it had any effect.
fn retry(mut call: impl FnMut() -> isize) -> Result<usize> {
    loop {
        let n = call();
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = last();
        if e.os != Some(libc::EINTR) {
            return Err(e);
        }
    }
}
pub(in crate::fs) fn bad_descriptor() -> Error {
    os(libc::EBADF)
}
fn offset(value: u64) -> Result<libc::off_t> {
    value.try_into().map_err(|_| os(libc::EINVAL))
}
fn file(object: Option<&mut Object>) -> Result<RawFd> {
    match object {
        Some(Object::File(f)) => Ok(f.0.as_raw_fd()),
        Some(Object::Dir(d)) => {
            // SAFETY: live stream owned by this handle.
            Ok(unsafe { libc::dirfd(d.0.as_ptr()) })
        }
        _ => Err(bad_descriptor()),
    }
}

pub(in crate::fs) fn open(path: &FsPath, o: &FileOptions) -> Result<File> {
    let writes = o.write || o.append;
    let mut flags = match (o.read, writes) {
        (true, false) => libc::O_RDONLY,
        (false, true) => libc::O_WRONLY,
        (true, true) => libc::O_RDWR,
        (false, false) => return Err(Error::new(ErrorKind::InvalidInput)),
    } | libc::O_CLOEXEC;
    if o.create || o.exclusive {
        flags |= libc::O_CREAT;
    }
    if o.exclusive {
        flags |= libc::O_EXCL;
    }
    if o.truncate {
        flags |= libc::O_TRUNC;
    }
    if o.append {
        flags |= libc::O_APPEND;
    }
    if o.sync {
        flags |= libc::O_SYNC;
    }
    if o.data_sync {
        flags |= libc::O_DSYNC;
    }
    if !o.follow_symlinks {
        flags |= libc::O_NOFOLLOW;
    }
    let fd = retry(|| {
        // SAFETY: NUL-terminated prepared path; integer flags and creation mode.
        unsafe { libc::open(path.native().as_ptr(), flags, o.mode as libc::c_uint) as isize }
    })?;
    // SAFETY: open returned a new descriptor owned by nobody else.
    Ok(File(unsafe { OwnedFd::from_raw_fd(fd as RawFd) }))
}
pub(in crate::fs) fn open_dir(path: &FsPath) -> Result<Dir> {
    // SAFETY: NUL-terminated prepared path.
    let stream = unsafe { libc::opendir(path.native().as_ptr()) };
    NonNull::new(stream).map(Dir).ok_or_else(last)
}
pub(in crate::fs) fn close(file: File) -> Result<()> {
    // SAFETY: the owned descriptor is closed exactly once. EINTR is not retried:
    // the descriptor is released either way on Linux and macOS.
    check(unsafe { libc::close(file.0.into_raw_fd()) })
}

pub(in crate::fs) fn execute(request: FsRequest, object: Option<&mut Object>) -> Result<FsOutput> {
    match request {
        FsRequest::Read {
            mut buffer,
            offset: at,
            ..
        } => {
            let fd = file(object)?;
            let at = at.map(offset).transpose()?;
            let out = output(&mut buffer);
            retry(|| {
                // SAFETY: exclusive, live output region; descriptor owned by this handle.
                unsafe {
                    match at {
                        Some(at) => libc::pread(fd, out.as_mut_ptr().cast(), out.len(), at),
                        None => libc::read(fd, out.as_mut_ptr().cast(), out.len()),
                    }
                }
            })
            .map(FsOutput::Read)
        }
        FsRequest::Write {
            buffer, offset: at, ..
        } => {
            let fd = file(object)?;
            let at = at.map(offset).transpose()?;
            let bytes = buffer.as_slice();
            retry(|| {
                // SAFETY: initialized input kept alive by the accepted request.
                unsafe {
                    match at {
                        Some(at) => libc::pwrite(fd, bytes.as_ptr().cast(), bytes.len(), at),
                        None => libc::write(fd, bytes.as_ptr().cast(), bytes.len()),
                    }
                }
            })
            .map(FsOutput::Wrote)
        }
        FsRequest::Sync { data_only, .. } => {
            let fd = file(object)?;
            sync(fd, data_only)?;
            Ok(FsOutput::Done)
        }
        FsRequest::Truncate { size, .. } => {
            let fd = file(object)?;
            let size = offset(size)?;
            // SAFETY: owned descriptor and representable length.
            retry(|| unsafe { libc::ftruncate(fd, size) as isize })?;
            Ok(FsOutput::Done)
        }
        FsRequest::Stat {
            path,
            follow_symlinks,
        } => stat_path(path.native(), follow_symlinks).map(FsOutput::Metadata),
        FsRequest::Fstat { .. } => stat_fd(file(object)?).map(FsOutput::Metadata),
        FsRequest::ReadDir { mut buffer, .. } => {
            let Some(Object::Dir(dir)) = object else {
                return Err(bad_descriptor());
            };
            read_dir(dir, output(&mut buffer))
        }
        FsRequest::Mkdir { path, mode } => {
            // SAFETY: prepared path and mode bits.
            check(unsafe { libc::mkdir(path.native().as_ptr(), mode as libc::mode_t) })
                .map(|()| FsOutput::Done)
        }
        FsRequest::Rmdir { path } => {
            // SAFETY: prepared path.
            check(unsafe { libc::rmdir(path.native().as_ptr()) }).map(|()| FsOutput::Done)
        }
        FsRequest::Unlink { path } => {
            // SAFETY: prepared path.
            check(unsafe { libc::unlink(path.native().as_ptr()) }).map(|()| FsOutput::Done)
        }
        FsRequest::Rename { from, to } => {
            // SAFETY: both prepared paths live through the call.
            check(unsafe { libc::rename(from.native().as_ptr(), to.native().as_ptr()) })
                .map(|()| FsOutput::Done)
        }
        FsRequest::Link { existing, link } => {
            // SAFETY: both prepared paths live through the call.
            check(unsafe { libc::link(existing.native().as_ptr(), link.native().as_ptr()) })
                .map(|()| FsOutput::Done)
        }
        FsRequest::Symlink { target, link, .. } => {
            // SAFETY: both prepared paths live through the call.
            check(unsafe { libc::symlink(target.native().as_ptr(), link.native().as_ptr()) })
                .map(|()| FsOutput::Done)
        }
        FsRequest::ReadLink { path, mut buffer } => {
            let out = output(&mut buffer);
            // SAFETY: prepared path and exclusive output region.
            let n = unsafe {
                libc::readlink(path.native().as_ptr(), out.as_mut_ptr().cast(), out.len())
            };
            if n < 0 {
                return Err(last());
            }
            if n as usize == out.len() {
                // The content may have been truncated to the buffer.
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
            Ok(FsOutput::Bytes(n as usize))
        }
        FsRequest::RealPath { path, mut buffer } => {
            let mut resolved = [0 as libc::c_char; libc::PATH_MAX as usize + 1];
            // SAFETY: the resolved buffer holds PATH_MAX bytes plus a NUL, as required.
            if unsafe { libc::realpath(path.native().as_ptr(), resolved.as_mut_ptr()) }.is_null() {
                return Err(last());
            }
            // SAFETY: successful realpath NUL-terminated the buffer.
            let bytes = unsafe { CStr::from_ptr(resolved.as_ptr()) }.to_bytes();
            let out = output(&mut buffer);
            if bytes.len() > out.len() {
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
            out[..bytes.len()].copy_from_slice(bytes);
            Ok(FsOutput::Bytes(bytes.len()))
        }
        FsRequest::Access { path, mode } => {
            let AccessMode {
                read,
                write,
                execute,
            } = mode;
            let bits = if read { libc::R_OK } else { 0 }
                | if write { libc::W_OK } else { 0 }
                | if execute { libc::X_OK } else { 0 };
            let bits = if bits == 0 { libc::F_OK } else { bits };
            // SAFETY: prepared path and access bits.
            check(unsafe { libc::access(path.native().as_ptr(), bits) }).map(|()| FsOutput::Done)
        }
        FsRequest::Chmod { target, mode } => {
            let mode = mode as libc::mode_t;
            // SAFETY: prepared path or owned descriptor, and mode bits.
            check(unsafe {
                match &target {
                    FsTarget::Path {
                        path,
                        follow_symlinks: true,
                    } => libc::chmod(path.native().as_ptr(), mode),
                    FsTarget::Path { path, .. } => libc::fchmodat(
                        libc::AT_FDCWD,
                        path.native().as_ptr(),
                        mode,
                        libc::AT_SYMLINK_NOFOLLOW,
                    ),
                    FsTarget::File(_) => libc::fchmod(file(object)?, mode),
                }
            })
            .map(|()| FsOutput::Done)
        }
        FsRequest::Chown { target, uid, gid } => {
            let uid = uid.map_or(libc::uid_t::MAX, |v| v as libc::uid_t);
            let gid = gid.map_or(libc::gid_t::MAX, |v| v as libc::gid_t);
            // SAFETY: prepared path or owned descriptor, and scalar ids (-1 keeps).
            check(unsafe {
                match &target {
                    FsTarget::Path {
                        path,
                        follow_symlinks: true,
                    } => libc::chown(path.native().as_ptr(), uid, gid),
                    FsTarget::Path { path, .. } => libc::lchown(path.native().as_ptr(), uid, gid),
                    FsTarget::File(_) => libc::fchown(file(object)?, uid, gid),
                }
            })
            .map(|()| FsOutput::Done)
        }
        FsRequest::SetTimes {
            target,
            accessed,
            modified,
        } => {
            let times = [timespec(accessed), timespec(modified)];
            // SAFETY: two initialized timespecs; prepared path or owned descriptor.
            check(unsafe {
                match &target {
                    FsTarget::Path {
                        path,
                        follow_symlinks,
                    } => libc::utimensat(
                        libc::AT_FDCWD,
                        path.native().as_ptr(),
                        times.as_ptr(),
                        if *follow_symlinks {
                            0
                        } else {
                            libc::AT_SYMLINK_NOFOLLOW
                        },
                    ),
                    FsTarget::File(_) => libc::futimens(file(object)?, times.as_ptr()),
                }
            })
            .map(|()| FsOutput::Done)
        }
        FsRequest::CopyFile {
            from,
            to,
            exclusive,
        } => copy_file(&from, &to, exclusive).map(|()| FsOutput::Done),
        FsRequest::Open { .. } | FsRequest::OpenDir { .. } | FsRequest::Close { .. } => {
            unreachable!("handled by the service")
        }
    }
}

fn sync(fd: RawFd, data_only: bool) -> Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        let _ = data_only;
        // As libuv: fsync does not flush the drive cache on Apple platforms.
        // SAFETY: owned descriptor, integer command.
        if unsafe { libc::fcntl(fd, libc::F_FULLFSYNC) } == 0 {
            return Ok(());
        }
        // SAFETY: owned descriptor.
        retry(|| unsafe { libc::fsync(fd) as isize }).map(drop)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        // SAFETY: owned descriptor.
        retry(|| unsafe {
            if data_only {
                libc::fdatasync(fd) as isize
            } else {
                libc::fsync(fd) as isize
            }
        })
        .map(drop)
    }
}

fn timespec(change: TimeChange) -> libc::timespec {
    match change {
        TimeChange::Keep => libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_OMIT,
        },
        TimeChange::Now => libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_NOW,
        },
        TimeChange::At(t) => libc::timespec {
            tv_sec: t.seconds as libc::time_t,
            tv_nsec: t.nanoseconds as _,
        },
    }
}
fn time(seconds: i64, nanoseconds: i64) -> Option<FileTime> {
    Some(FileTime {
        seconds,
        nanoseconds: nanoseconds as u32,
    })
}
// mode_t is u16 on Apple platforms and u32 elsewhere.
#[allow(clippy::unnecessary_cast)]
fn kind(mode: u32) -> FileType {
    match mode & libc::S_IFMT as u32 {
        m if m == libc::S_IFREG as u32 => FileType::File,
        m if m == libc::S_IFDIR as u32 => FileType::Directory,
        m if m == libc::S_IFLNK as u32 => FileType::Symlink,
        m if m == libc::S_IFBLK as u32 => FileType::BlockDevice,
        m if m == libc::S_IFCHR as u32 => FileType::CharDevice,
        m if m == libc::S_IFIFO as u32 => FileType::Fifo,
        m if m == libc::S_IFSOCK as u32 => FileType::Socket,
        _ => FileType::Unknown,
    }
}
#[allow(clippy::unnecessary_cast)]
fn from_stat(s: &libc::stat) -> FileMetadata {
    FileMetadata {
        kind: kind(s.st_mode as u32),
        size: s.st_size as u64,
        mode: Some(s.st_mode as u32),
        device: Some(s.st_dev as u64),
        inode: Some(s.st_ino as u64),
        links: Some(s.st_nlink as u64),
        uid: Some(s.st_uid as u32),
        gid: Some(s.st_gid as u32),
        rdev: Some(s.st_rdev as u64),
        block_size: Some(s.st_blksize as u64),
        blocks: Some(s.st_blocks as u64),
        accessed: time(s.st_atime as i64, s.st_atime_nsec as i64),
        modified: time(s.st_mtime as i64, s.st_mtime_nsec as i64),
        changed: time(s.st_ctime as i64, s.st_ctime_nsec as i64),
        #[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
        created: time(s.st_birthtime as i64, s.st_birthtime_nsec as i64),
        #[cfg(not(any(target_vendor = "apple", target_os = "freebsd")))]
        created: None,
    }
}
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn statx(fd: RawFd, path: &CStr, flags: c_int) -> Option<Result<FileMetadata>> {
    let mut s = std::mem::MaybeUninit::<libc::statx>::uninit();
    // SAFETY: valid directory descriptor or AT_FDCWD, NUL-terminated path, writable output.
    let r = unsafe {
        libc::statx(
            fd,
            path.as_ptr(),
            flags,
            libc::STATX_BASIC_STATS | libc::STATX_BTIME,
            s.as_mut_ptr(),
        )
    };
    if r < 0 {
        let e = last();
        // Old kernels and seccomp filters: use the stat family instead.
        if matches!(e.os, Some(libc::ENOSYS | libc::EPERM)) {
            return None;
        }
        return Some(Err(e));
    }
    // SAFETY: successful statx initialized the structure.
    let s = unsafe { s.assume_init() };
    let t = |t: libc::statx_timestamp| time(t.tv_sec, i64::from(t.tv_nsec));
    let mode = u32::from(s.stx_mode);
    Some(Ok(FileMetadata {
        kind: kind(mode),
        size: s.stx_size,
        mode: Some(mode),
        device: Some(libc::makedev(s.stx_dev_major, s.stx_dev_minor)),
        inode: Some(s.stx_ino),
        links: Some(u64::from(s.stx_nlink)),
        uid: Some(s.stx_uid),
        gid: Some(s.stx_gid),
        rdev: Some(libc::makedev(s.stx_rdev_major, s.stx_rdev_minor)),
        block_size: Some(u64::from(s.stx_blksize)),
        blocks: Some(s.stx_blocks),
        accessed: t(s.stx_atime),
        modified: t(s.stx_mtime),
        changed: t(s.stx_ctime),
        created: (s.stx_mask & libc::STATX_BTIME != 0)
            .then(|| t(s.stx_btime))
            .flatten(),
    }))
}
fn stat_path(path: &CStr, follow: bool) -> Result<FileMetadata> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if let Some(result) = statx(
        libc::AT_FDCWD,
        path,
        if follow { 0 } else { libc::AT_SYMLINK_NOFOLLOW },
    ) {
        return result;
    }
    let mut s = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: NUL-terminated path and writable output, read only after success.
    check(unsafe {
        if follow {
            libc::stat(path.as_ptr(), s.as_mut_ptr())
        } else {
            libc::lstat(path.as_ptr(), s.as_mut_ptr())
        }
    })?;
    // SAFETY: successful stat initialized the structure.
    Ok(from_stat(unsafe { s.assume_init_ref() }))
}
fn stat_fd(fd: RawFd) -> Result<FileMetadata> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if let Some(result) = statx(fd, c"", libc::AT_EMPTY_PATH) {
        return result;
    }
    let mut s = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: owned descriptor and writable output, read only after success.
    check(unsafe { libc::fstat(fd, s.as_mut_ptr()) })?;
    // SAFETY: successful fstat initialized the structure.
    Ok(from_stat(unsafe { s.assume_init_ref() }))
}

fn set_errno(value: c_int) {
    // SAFETY: the thread-local errno location is valid for this thread.
    unsafe {
        #[cfg(target_os = "linux")]
        {
            *libc::__errno_location() = value;
        }
        #[cfg(target_os = "android")]
        {
            // Bionic has no `__errno_location`; its errno accessor is `__errno`.
            *libc::__errno() = value;
        }
        #[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
        {
            *libc::__error() = value;
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_vendor = "apple",
            target_os = "freebsd"
        )))]
        {
            *libc::__errno() = value;
        }
    }
}
fn read_dir(dir: &mut Dir, out: &mut [u8]) -> Result<FsOutput> {
    let stream = dir.0.as_ptr();
    let mut used = 0;
    loop {
        // SAFETY: live stream exclusively used by this request.
        let position = unsafe { libc::telldir(stream) };
        set_errno(0);
        // SAFETY: live stream; the entry is read before the next readdir call.
        let entry = unsafe { libc::readdir(stream) };
        let Some(entry) = NonNull::new(entry) else {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error().is_some_and(|code| code != 0) {
                return Err(e.into());
            }
            return Ok(FsOutput::Directory { n: used, eof: true });
        };
        // SAFETY: readdir returned a live entry with a NUL-terminated name.
        let (name, tag) = unsafe {
            let entry = entry.as_ref();
            (
                CStr::from_ptr(entry.d_name.as_ptr()).to_bytes(),
                entry.d_type,
            )
        };
        if name == b"." || name == b".." {
            continue;
        }
        let tag = match tag {
            libc::DT_REG => FileType::File,
            libc::DT_DIR => FileType::Directory,
            libc::DT_LNK => FileType::Symlink,
            libc::DT_BLK => FileType::BlockDevice,
            libc::DT_CHR => FileType::CharDevice,
            libc::DT_FIFO => FileType::Fifo,
            libc::DT_SOCK => FileType::Socket,
            _ => FileType::Unknown,
        };
        if !put_record(out, &mut used, tag as u8, name) {
            // Leave the entry for the next page.
            // SAFETY: position came from telldir on this live stream.
            unsafe { libc::seekdir(stream, position) };
            if used == 0 {
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
            return Ok(FsOutput::Directory {
                n: used,
                eof: false,
            });
        }
    }
}

fn copy_file(from: &FsPath, to: &FsPath, exclusive: bool) -> Result<()> {
    let source = open(
        from,
        &FileOptions {
            read: true,
            ..FileOptions::default()
        },
    )?;
    let metadata = stat_fd(source.0.as_raw_fd())?;
    if metadata.kind == FileType::Directory {
        return Err(os(libc::EISDIR));
    }
    let permissions = metadata.mode.unwrap_or(0o644) & 0o7777;
    let target = open(
        to,
        &FileOptions {
            read: false,
            write: true,
            create: true,
            exclusive,
            truncate: !exclusive,
            mode: permissions,
            ..FileOptions::default()
        },
    )?;
    let mut reader = std::fs::File::from(source.0);
    let mut writer = std::fs::File::from(target.0);
    // std uses copy_file_range/sendfile on Linux and a stack buffer elsewhere.
    std::io::copy(&mut reader, &mut writer).map_err(Error::from)?;
    // SAFETY: owned destination descriptor and the source's permission bits.
    check(unsafe { libc::fchmod(writer.as_raw_fd(), permissions as libc::mode_t) })
}
