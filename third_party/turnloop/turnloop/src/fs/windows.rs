//! Windows filesystem calls on synchronous handles, executed only on pool threads.
//! Positional I/O passes an OVERLAPPED offset to ReadFile/WriteFile; cursor I/O
//! keeps a per-handle cursor, serialized by the handle's FIFO.
use super::{Object, output};
use crate::{
    Error, ErrorKind, Result,
    fs::{
        AccessMode, FileMetadata, FileOptions, FileTime, FileType, FsOutput, FsPath, FsRequest,
        FsTarget, RECORD_HEADER, SymlinkKind, TimeChange, wtf8,
    },
};
use std::{
    mem::{size_of, zeroed},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::IO::{OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0},
};
use windows_sys::core::BOOL;

pub(in crate::fs) struct File {
    handle: OwnedHandle,
    cursor: u64,
    append: bool,
}
pub(in crate::fs) struct Dir {
    handle: OwnedHandle,
    pattern: Box<[u16]>,
    search: HANDLE,
    data: Box<WIN32_FIND_DATAW>,
    pending: bool,
    done: bool,
}
// SAFETY: the search handle is used by one pool thread at a time under the
// handle's mutex and closed exactly once by Drop.
unsafe impl Send for Dir {}
impl Drop for Dir {
    fn drop(&mut self) {
        if self.search != INVALID_HANDLE_VALUE {
            // SAFETY: live search handle owned by this directory stream.
            unsafe { FindClose(self.search) };
        }
    }
}

fn last() -> Error {
    std::io::Error::last_os_error().into()
}
fn os(code: u32) -> Error {
    std::io::Error::from_raw_os_error(code as i32).into()
}
fn check(ok: BOOL) -> Result<()> {
    if ok == 0 { Err(last()) } else { Ok(()) }
}
pub(in crate::fs) fn bad_descriptor() -> Error {
    os(ERROR_INVALID_HANDLE)
}
fn create(path: &FsPath, access: u32, disposition: u32, flags: u32) -> Result<OwnedHandle> {
    // SAFETY: NUL-terminated prepared path, default security and no template.
    let h = unsafe {
        CreateFileW(
            path.wide().as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            disposition,
            flags,
            ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(last());
    }
    // SAFETY: CreateFileW returned a new handle owned by nobody else.
    Ok(unsafe { OwnedHandle::from_raw_handle(h) })
}
fn raw(object: Option<&mut Object>) -> Result<HANDLE> {
    match object {
        Some(Object::File(f)) => Ok(f.handle.as_raw_handle()),
        Some(Object::Dir(d)) => Ok(d.handle.as_raw_handle()),
        _ => Err(bad_descriptor()),
    }
}
fn nofollow(follow: bool) -> u32 {
    if follow {
        0
    } else {
        FILE_FLAG_OPEN_REPARSE_POINT
    }
}

pub(in crate::fs) fn open(path: &FsPath, o: &FileOptions) -> Result<File> {
    let mut access = match (o.read, o.write || o.append) {
        (true, false) => FILE_GENERIC_READ,
        (false, true) => FILE_GENERIC_WRITE,
        (true, true) => FILE_GENERIC_READ | FILE_GENERIC_WRITE,
        (false, false) => return Err(Error::new(ErrorKind::InvalidInput)),
    };
    if o.append {
        access = (access & !FILE_WRITE_DATA) | FILE_APPEND_DATA;
    }
    // The disposition table follows libuv's fs__open.
    let disposition = match (o.create || o.exclusive, o.exclusive, o.truncate) {
        (true, true, _) => CREATE_NEW,
        (true, false, true) => CREATE_ALWAYS,
        (true, false, false) => OPEN_ALWAYS,
        (false, _, true) => TRUNCATE_EXISTING,
        (false, _, false) => OPEN_EXISTING,
    };
    let mut flags =
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS | nofollow(o.follow_symlinks);
    if (o.create || o.exclusive) && o.mode & 0o200 == 0 {
        flags |= FILE_ATTRIBUTE_READONLY;
    }
    if o.sync || o.data_sync {
        flags |= FILE_FLAG_WRITE_THROUGH;
    }
    Ok(File {
        handle: create(path, access, disposition, flags)?,
        cursor: 0,
        append: o.append,
    })
}
pub(in crate::fs) fn open_dir(path: &FsPath) -> Result<Dir> {
    let handle = create(
        path,
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS,
    )?;
    let wide = &path.wide()[..path.wide().len() - 1];
    let mut pattern = Vec::with_capacity(wide.len() + 3);
    pattern.extend_from_slice(wide);
    if !matches!(wide.last(), Some(&0x5c | &0x2f)) {
        pattern.push(0x5c);
    }
    pattern.extend_from_slice(&[0x2a, 0]);
    Ok(Dir {
        handle,
        pattern: pattern.into_boxed_slice(),
        search: INVALID_HANDLE_VALUE,
        data: Box::default(),
        pending: false,
        done: false,
    })
}
pub(in crate::fs) fn close(file: File) -> Result<()> {
    let handle = std::os::windows::io::IntoRawHandle::into_raw_handle(file.handle);
    // SAFETY: the owned handle is closed exactly once.
    check(unsafe { CloseHandle(handle) })
}

fn overlapped(offset: u64) -> OVERLAPPED {
    OVERLAPPED {
        Internal: 0,
        InternalHigh: 0,
        Anonymous: OVERLAPPED_0 {
            Anonymous: OVERLAPPED_0_0 {
                Offset: offset as u32,
                OffsetHigh: (offset >> 32) as u32,
            },
        },
        hEvent: ptr::null_mut(),
    }
}

pub(in crate::fs) fn execute(request: FsRequest, object: Option<&mut Object>) -> Result<FsOutput> {
    match request {
        FsRequest::Read {
            mut buffer, offset, ..
        } => {
            let Some(Object::File(file)) = object else {
                return Err(bad_descriptor());
            };
            let out = output(&mut buffer);
            let at = offset.unwrap_or(file.cursor);
            let mut ov = overlapped(at);
            let mut n = 0u32;
            let len = out.len().min(u32::MAX as usize) as u32;
            // SAFETY: synchronous handle, exclusive output region and a live
            // OVERLAPPED that only carries the offset for this call.
            if unsafe {
                ReadFile(
                    file.handle.as_raw_handle(),
                    out.as_mut_ptr(),
                    len,
                    &mut n,
                    &mut ov,
                )
            } == 0
            {
                let e = last();
                if e.os != Some(ERROR_HANDLE_EOF as i32) {
                    return Err(e);
                }
                n = 0;
            }
            if offset.is_none() {
                file.cursor += u64::from(n);
            }
            Ok(FsOutput::Read(n as usize))
        }
        FsRequest::Write { buffer, offset, .. } => {
            let Some(Object::File(file)) = object else {
                return Err(bad_descriptor());
            };
            let bytes = buffer.as_slice();
            // All-ones offsets append at the end of the file.
            let at = if file.append {
                u64::MAX
            } else {
                offset.unwrap_or(file.cursor)
            };
            let mut ov = overlapped(at);
            let mut n = 0u32;
            let len = bytes.len().min(u32::MAX as usize) as u32;
            // SAFETY: synchronous handle, initialized input kept alive by the request.
            check(unsafe {
                WriteFile(
                    file.handle.as_raw_handle(),
                    bytes.as_ptr(),
                    len,
                    &mut n,
                    &mut ov,
                )
            })?;
            if offset.is_none() && !file.append {
                file.cursor += u64::from(n);
            }
            Ok(FsOutput::Wrote(n as usize))
        }
        FsRequest::Sync { .. } => {
            // SAFETY: live owned handle.
            check(unsafe { FlushFileBuffers(raw(object)?) }).map(|()| FsOutput::Done)
        }
        FsRequest::Truncate { size, .. } => {
            let info = FILE_END_OF_FILE_INFO {
                EndOfFile: i64::try_from(size).map_err(|_| os(ERROR_INVALID_PARAMETER))?,
            };
            // SAFETY: live handle and a correctly sized information record.
            check(unsafe {
                SetFileInformationByHandle(
                    raw(object)?,
                    FileEndOfFileInfo,
                    (&info as *const FILE_END_OF_FILE_INFO).cast(),
                    size_of::<FILE_END_OF_FILE_INFO>() as u32,
                )
            })
            .map(|()| FsOutput::Done)
        }
        FsRequest::Stat {
            path,
            follow_symlinks,
        } => {
            let h = create(
                &path,
                FILE_READ_ATTRIBUTES,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | nofollow(follow_symlinks),
            )?;
            metadata(h.as_raw_handle()).map(FsOutput::Metadata)
        }
        FsRequest::Fstat { .. } => metadata(raw(object)?).map(FsOutput::Metadata),
        FsRequest::ReadDir { mut buffer, .. } => {
            let Some(Object::Dir(dir)) = object else {
                return Err(bad_descriptor());
            };
            read_dir(dir, output(&mut buffer))
        }
        FsRequest::Mkdir { path, .. } => {
            // SAFETY: prepared path, default security.
            check(unsafe { CreateDirectoryW(path.wide().as_ptr(), ptr::null()) })
                .map(|()| FsOutput::Done)
        }
        FsRequest::Rmdir { path } => {
            // SAFETY: prepared path.
            check(unsafe { RemoveDirectoryW(path.wide().as_ptr()) }).map(|()| FsOutput::Done)
        }
        FsRequest::Unlink { path } => {
            // SAFETY: prepared path.
            check(unsafe { DeleteFileW(path.wide().as_ptr()) }).map(|()| FsOutput::Done)
        }
        FsRequest::Rename { from, to } => {
            // SAFETY: both prepared paths live through the call.
            check(unsafe {
                MoveFileExW(
                    from.wide().as_ptr(),
                    to.wide().as_ptr(),
                    MOVEFILE_REPLACE_EXISTING,
                )
            })
            .map(|()| FsOutput::Done)
        }
        FsRequest::Link { existing, link } => {
            // SAFETY: both prepared paths live through the call.
            check(unsafe {
                CreateHardLinkW(link.wide().as_ptr(), existing.wide().as_ptr(), ptr::null())
            })
            .map(|()| FsOutput::Done)
        }
        FsRequest::Symlink { target, link, kind } => {
            let base = if kind == SymlinkKind::Directory {
                SYMBOLIC_LINK_FLAG_DIRECTORY
            } else {
                0
            };
            let create = |flags| {
                // SAFETY: both prepared paths live through the call.
                unsafe { CreateSymbolicLinkW(link.wide().as_ptr(), target.wide().as_ptr(), flags) }
            };
            // Unprivileged creation needs developer mode; older systems reject the flag.
            if create(base | SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE) {
                return Ok(FsOutput::Done);
            }
            let e = last();
            if e.os != Some(ERROR_INVALID_PARAMETER as i32) {
                return Err(e);
            }
            if create(base) {
                Ok(FsOutput::Done)
            } else {
                Err(last())
            }
        }
        FsRequest::ReadLink { path, mut buffer } => {
            use std::os::windows::ffi::OsStrExt;
            let content = std::fs::read_link(path.as_path()).map_err(Error::from)?;
            let units: Vec<u16> = content.as_os_str().encode_wide().collect();
            let out = output(&mut buffer);
            let mut used = 0;
            if !wtf8(&units, out, &mut used) {
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
            Ok(FsOutput::Bytes(used))
        }
        FsRequest::RealPath { path, mut buffer } => {
            let h = create(
                &path,
                FILE_READ_ATTRIBUTES,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
            )?;
            let mut wide = [0u16; 32768];
            // SAFETY: live handle and writable output of the stated length.
            let n = unsafe {
                GetFinalPathNameByHandleW(
                    h.as_raw_handle(),
                    wide.as_mut_ptr(),
                    wide.len() as u32,
                    FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
                )
            } as usize;
            if n == 0 {
                return Err(last());
            }
            if n >= wide.len() {
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
            let full = &wide[..n];
            let out = output(&mut buffer);
            let mut used = 0;
            const VERBATIM: &[u16] = &[0x5c, 0x5c, 0x3f, 0x5c];
            const UNC: &[u16] = &[0x55, 0x4e, 0x43, 0x5c];
            let fits = if let Some(rest) = full.strip_prefix(VERBATIM) {
                if let Some(share) = rest.strip_prefix(UNC) {
                    wtf8(&[0x5c, 0x5c], out, &mut used) && wtf8(share, out, &mut used)
                } else {
                    wtf8(rest, out, &mut used)
                }
            } else {
                wtf8(full, out, &mut used)
            };
            if !fits {
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
            Ok(FsOutput::Bytes(used))
        }
        FsRequest::Access { path, mode } => {
            // SAFETY: prepared path.
            let attributes = unsafe { GetFileAttributesW(path.wide().as_ptr()) };
            if attributes == INVALID_FILE_ATTRIBUTES {
                return Err(last());
            }
            let AccessMode { write, .. } = mode;
            if write
                && attributes & FILE_ATTRIBUTE_READONLY != 0
                && attributes & FILE_ATTRIBUTE_DIRECTORY == 0
            {
                return Err(os(ERROR_ACCESS_DENIED));
            }
            Ok(FsOutput::Done)
        }
        FsRequest::Chmod { target, mode } => {
            let readonly = mode & 0o200 == 0;
            let toggle = |attributes: u32| {
                let next = if readonly {
                    attributes | FILE_ATTRIBUTE_READONLY
                } else {
                    attributes & !FILE_ATTRIBUTE_READONLY
                };
                if next == 0 {
                    FILE_ATTRIBUTE_NORMAL
                } else {
                    next
                }
            };
            match target {
                FsTarget::Path { path, .. } => {
                    // SAFETY: prepared path.
                    let attributes = unsafe { GetFileAttributesW(path.wide().as_ptr()) };
                    if attributes == INVALID_FILE_ATTRIBUTES {
                        return Err(last());
                    }
                    // SAFETY: prepared path and attribute bits.
                    check(unsafe { SetFileAttributesW(path.wide().as_ptr(), toggle(attributes)) })?;
                }
                FsTarget::File(_) => {
                    let h = raw(object)?;
                    // SAFETY: FILE_BASIC_INFO is plain integers.
                    let mut basic: FILE_BASIC_INFO = unsafe { zeroed() };
                    // SAFETY: live handle and a correctly sized output record.
                    check(unsafe {
                        GetFileInformationByHandleEx(
                            h,
                            FileBasicInfo,
                            (&mut basic as *mut FILE_BASIC_INFO).cast(),
                            size_of::<FILE_BASIC_INFO>() as u32,
                        )
                    })?;
                    let update = FILE_BASIC_INFO {
                        CreationTime: 0,
                        LastAccessTime: 0,
                        LastWriteTime: 0,
                        ChangeTime: 0,
                        FileAttributes: toggle(basic.FileAttributes),
                    };
                    // SAFETY: live handle; zero times leave the timestamps unchanged.
                    check(unsafe {
                        SetFileInformationByHandle(
                            h,
                            FileBasicInfo,
                            (&update as *const FILE_BASIC_INFO).cast(),
                            size_of::<FILE_BASIC_INFO>() as u32,
                        )
                    })?;
                }
            }
            Ok(FsOutput::Done)
        }
        FsRequest::Chown { .. } => Err(Error::new(ErrorKind::Unsupported)),
        FsRequest::SetTimes {
            target,
            accessed,
            modified,
        } => {
            let opened;
            let h = match &target {
                FsTarget::Path {
                    path,
                    follow_symlinks,
                } => {
                    opened = create(
                        path,
                        FILE_WRITE_ATTRIBUTES,
                        OPEN_EXISTING,
                        FILE_FLAG_BACKUP_SEMANTICS | nofollow(*follow_symlinks),
                    )?;
                    opened.as_raw_handle()
                }
                FsTarget::File(_) => raw(object)?,
            };
            let a = filetime(accessed);
            let m = filetime(modified);
            let pointer = |t: &Option<FILETIME>| t.as_ref().map_or(ptr::null(), |t| t as *const _);
            // SAFETY: live handle; null pointers leave a timestamp unchanged.
            check(unsafe { SetFileTime(h, ptr::null(), pointer(&a), pointer(&m)) })
                .map(|()| FsOutput::Done)
        }
        FsRequest::CopyFile {
            from,
            to,
            exclusive,
        } => {
            // SAFETY: both prepared paths live through the call.
            check(unsafe {
                CopyFileW(
                    from.wide().as_ptr(),
                    to.wide().as_ptr(),
                    BOOL::from(exclusive),
                )
            })
            .map(|()| FsOutput::Done)
        }
        FsRequest::Open { .. } | FsRequest::OpenDir { .. } | FsRequest::Close { .. } => {
            unreachable!("handled by the service")
        }
    }
}

const EPOCH_DIFFERENCE: i64 = 11_644_473_600;
fn from_ticks(ticks: i64) -> Option<FileTime> {
    (ticks != 0).then(|| FileTime {
        seconds: ticks.div_euclid(10_000_000) - EPOCH_DIFFERENCE,
        nanoseconds: (ticks.rem_euclid(10_000_000) * 100) as u32,
    })
}
fn ticks(t: FILETIME) -> i64 {
    ((u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)) as i64
}
fn filetime(change: TimeChange) -> Option<FILETIME> {
    let ticks = match change {
        TimeChange::Keep => return None,
        TimeChange::Now => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            (now.as_secs() as i64 + EPOCH_DIFFERENCE) * 10_000_000
                + i64::from(now.subsec_nanos() / 100)
        }
        TimeChange::At(t) => {
            (t.seconds + EPOCH_DIFFERENCE) * 10_000_000 + i64::from(t.nanoseconds / 100)
        }
    } as u64;
    Some(FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    })
}
/// Name-surrogate reparse points (symbolic links and junctions) read as links.
fn is_link(h: HANDLE, attributes: u32) -> bool {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return false;
    }
    // SAFETY: plain integer record.
    let mut tag: FILE_ATTRIBUTE_TAG_INFO = unsafe { zeroed() };
    // SAFETY: live handle and a correctly sized output record.
    unsafe {
        GetFileInformationByHandleEx(
            h,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        ) != 0
            && tag.ReparseTag & 0x2000_0000 != 0
    }
}
fn metadata(h: HANDLE) -> Result<FileMetadata> {
    // SAFETY: plain integer record.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    // SAFETY: live handle and writable output record.
    if unsafe { GetFileInformationByHandle(h, &mut info) } == 0 {
        let e = last();
        // SAFETY: live handle.
        let kind = match unsafe { GetFileType(h) } {
            FILE_TYPE_CHAR => FileType::CharDevice,
            FILE_TYPE_PIPE => FileType::Fifo,
            _ => return Err(e),
        };
        return Ok(FileMetadata {
            kind,
            size: 0,
            mode: Some(if kind == FileType::CharDevice {
                0o020666
            } else {
                0o010666
            }),
            device: None,
            inode: None,
            links: None,
            uid: None,
            gid: None,
            rdev: None,
            block_size: None,
            blocks: None,
            accessed: None,
            modified: None,
            changed: None,
            created: None,
        });
    }
    // SAFETY: plain integer record.
    let mut basic: FILE_BASIC_INFO = unsafe { zeroed() };
    // SAFETY: live handle and a correctly sized output record.
    let basic = (unsafe {
        GetFileInformationByHandleEx(
            h,
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } != 0)
        .then_some(basic);
    let attributes = info.dwFileAttributes;
    let kind = if is_link(h, attributes) {
        FileType::Symlink
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        FileType::Directory
    } else {
        FileType::File
    };
    let type_bits = match kind {
        FileType::Symlink => 0o120000,
        FileType::Directory => 0o040000,
        _ => 0o100000,
    };
    let permissions = if attributes & FILE_ATTRIBUTE_READONLY != 0 {
        0o444
    } else {
        0o666
    };
    Ok(FileMetadata {
        kind,
        size: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
        mode: Some(type_bits | permissions),
        device: Some(u64::from(info.dwVolumeSerialNumber)),
        inode: Some((u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)),
        links: Some(u64::from(info.nNumberOfLinks)),
        uid: None,
        gid: None,
        rdev: None,
        block_size: None,
        blocks: None,
        accessed: from_ticks(ticks(info.ftLastAccessTime)),
        modified: from_ticks(ticks(info.ftLastWriteTime)),
        changed: basic.and_then(|b| from_ticks(b.ChangeTime)),
        created: from_ticks(ticks(info.ftCreationTime)),
    })
}

fn read_dir(dir: &mut Dir, out: &mut [u8]) -> Result<FsOutput> {
    let mut used = 0;
    loop {
        if dir.done {
            return Ok(FsOutput::Directory { n: used, eof: true });
        }
        if dir.search == INVALID_HANDLE_VALUE {
            // SAFETY: NUL-terminated pattern and writable find record.
            let search = unsafe { FindFirstFileW(dir.pattern.as_ptr(), &mut *dir.data) };
            if search == INVALID_HANDLE_VALUE {
                let e = last();
                if e.os == Some(ERROR_FILE_NOT_FOUND as i32) {
                    dir.done = true;
                    continue;
                }
                return Err(e);
            }
            dir.search = search;
            dir.pending = true;
        }
        if !dir.pending {
            // SAFETY: live search handle and writable find record.
            if unsafe { FindNextFileW(dir.search, &mut *dir.data) } == 0 {
                let e = last();
                if e.os != Some(ERROR_NO_MORE_FILES as i32) {
                    return Err(e);
                }
                dir.done = true;
                continue;
            }
            dir.pending = true;
        }
        let data = &*dir.data;
        let length = data
            .cFileName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(data.cFileName.len());
        let name = &data.cFileName[..length];
        if name == [0x2e] || name == [0x2e, 0x2e] {
            dir.pending = false;
            continue;
        }
        let attributes = data.dwFileAttributes;
        let tag = if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            && data.dwReserved0 & 0x2000_0000 != 0
        {
            FileType::Symlink
        } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            FileType::Directory
        } else {
            FileType::File
        };
        let start = used;
        let mut end = used + RECORD_HEADER;
        let fits = end <= out.len()
            && wtf8(name, out, &mut end)
            && end - start - RECORD_HEADER <= usize::from(u16::MAX);
        if !fits {
            if used == 0 {
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
            return Ok(FsOutput::Directory {
                n: used,
                eof: false,
            });
        }
        out[start] = tag as u8;
        out[start + 1] = 0;
        let len = (end - start - RECORD_HEADER) as u16;
        out[start + 2..start + 4].copy_from_slice(&len.to_le_bytes());
        used = end;
        dir.pending = false;
    }
}
