//! Typed filesystem requests, native watches and their portable byte records.
//!
//! * **Native** (Linux, macOS/BSD, Windows): every request runs on the shared
//!   bounded blocking pool (DESIGN D8). Operations on one file or directory handle
//!   run in submission order; independent path requests may complete in any order.
//!   Adopted descriptors (`Detached::from_fd`/`from_handle`, stdio) keep their stream
//!   paths: reusable pool jobs on Unix, one synchronous worker per handle on Windows.
//! * **WASI 0.2/0.3**: the same requests map onto `wasi:filesystem`, resolved only
//!   against preopened directories. There is no ambient path authority.
//! * **Web**: Unsupported. No OPFS host mapping is defined.
//!
//! Every accepted request completes exactly once with `OpResult::Fs`, `Err`,
//! or `Cancelled`. A request that has started on a pool thread runs to its end;
//! cancellation then reports `Cancelled` and discards its result. Buffers follow
//! the D3 contract: provided memory is accessed only until the terminal completion.
use crate::{BufLease, Handle, ReadBuf, Result, WriteBuf};
use std::{path::Path, sync::Arc};

#[derive(Debug)]
struct PathInner {
    path: std::path::PathBuf,
    #[cfg(unix)]
    native: std::ffi::CString,
    #[cfg(windows)]
    native: Box<[u16]>,
    #[cfg(target_os = "wasi")]
    text: Box<str>,
}

#[derive(Clone, Debug)]
/// A validated, prepared pathname. Preparing converts once; cloning never allocates.
///
/// On WASI an absolute path resolves against the preopened directory with the
/// longest matching name, and a relative path against a preopen named `.`.
/// Paths outside every preopen fail with `NotFound`; the host enforces confinement.
pub struct FsPath(Arc<PathInner>);
impl FsPath {
    /// Prepare a pathname. Embedded NULs (and non-UTF-8 names on WASI) are InvalidInput.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        #[cfg(unix)]
        let native = {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| invalid())?
        };
        #[cfg(windows)]
        let native = {
            use std::os::windows::ffi::OsStrExt;
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            if wide.contains(&0) {
                return Err(invalid());
            }
            wide.push(0);
            wide.into_boxed_slice()
        };
        #[cfg(target_os = "wasi")]
        let text = {
            let text = path.to_str().ok_or_else(invalid)?;
            if text.contains('\0') {
                return Err(invalid());
            }
            text.into()
        };
        #[cfg(not(any(unix, windows, target_os = "wasi")))]
        if path.to_str().is_none_or(|s| s.contains('\0')) {
            return Err(invalid());
        }
        Ok(Self(Arc::new(PathInner {
            path: path.to_path_buf(),
            #[cfg(any(unix, windows))]
            native,
            #[cfg(target_os = "wasi")]
            text,
        })))
    }
    /// The original pathname, for host diagnostics and error messages.
    pub fn as_path(&self) -> &Path {
        &self.0.path
    }
    #[cfg(unix)]
    pub(crate) fn native(&self) -> &std::ffi::CStr {
        &self.0.native
    }
    #[cfg(windows)]
    pub(crate) fn wide(&self) -> &[u16] {
        &self.0.native
    }
    #[cfg(target_os = "wasi")]
    #[allow(dead_code)]
    pub(crate) fn text(&self) -> &str {
        &self.0.text
    }
}
fn invalid() -> crate::Error {
    crate::Error::new(crate::ErrorKind::InvalidInput)
}

#[derive(Clone, Copy, Debug)]
/// Open options covering Node's string and numeric open flags.
pub struct FileOptions {
    /// Allow reads.
    pub read: bool,
    /// Allow writes.
    pub write: bool,
    /// Create the file when it is absent.
    pub create: bool,
    /// Fail with AlreadyExists when the file exists (implies create).
    pub exclusive: bool,
    /// Truncate an existing file at open.
    pub truncate: bool,
    /// Every write appends, including writes that name an offset (as on Linux).
    pub append: bool,
    /// Synchronous file-integrity writes (O_SYNC; FILE_FLAG_WRITE_THROUGH on Windows).
    pub sync: bool,
    /// Synchronous data-integrity writes (O_DSYNC; FILE_FLAG_WRITE_THROUGH on Windows).
    pub data_sync: bool,
    /// Follow a final symbolic link; false fails on a link (O_NOFOLLOW).
    pub follow_symlinks: bool,
    /// Creation permissions filtered by the umask. Windows maps a missing owner
    /// write bit to the read-only attribute; WASI uses host defaults.
    pub mode: u32,
}
impl Default for FileOptions {
    fn default() -> Self {
        Self {
            read: true,
            write: false,
            create: false,
            exclusive: false,
            truncate: false,
            append: false,
            sync: false,
            data_sync: false,
            follow_symlinks: true,
            mode: 0o666,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Native file type; also the record tag of directory entries.
pub enum FileType {
    /// The native API did not report a type.
    Unknown = 0,
    /// Regular file.
    File = 1,
    /// Directory.
    Directory = 2,
    /// Symbolic link (or a Windows name-surrogate reparse point).
    Symlink = 3,
    /// Block device.
    BlockDevice = 4,
    /// Character device.
    CharDevice = 5,
    /// Named pipe.
    Fifo = 6,
    /// Local socket.
    Socket = 7,
}
impl FileType {
    fn from_tag(tag: u8) -> Self {
        match tag {
            1 => Self::File,
            2 => Self::Directory,
            3 => Self::Symlink,
            4 => Self::BlockDevice,
            5 => Self::CharDevice,
            6 => Self::Fifo,
            7 => Self::Socket,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
/// Wall-clock timestamp relative to the Unix epoch, with nanoseconds retained.
pub struct FileTime {
    /// Signed whole seconds from the Unix epoch.
    pub seconds: i64,
    /// Fractional nanoseconds, below one billion.
    pub nanoseconds: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Metadata for Node `Stats` and `watchFile` snapshots. A field the platform does
/// not report is None; values are never invented. Windows `mode` follows libuv's
/// attribute mapping: type bits plus 0o666, or 0o444 when read-only.
pub struct FileMetadata {
    /// File type.
    pub kind: FileType,
    /// Length in bytes.
    pub size: u64,
    /// Mode including type bits.
    pub mode: Option<u32>,
    /// Device (Windows: volume serial number).
    pub device: Option<u64>,
    /// Inode (Windows: file index).
    pub inode: Option<u64>,
    /// Hard-link count.
    pub links: Option<u64>,
    /// Owner user id.
    pub uid: Option<u32>,
    /// Owner group id.
    pub gid: Option<u32>,
    /// Device id of a special file.
    pub rdev: Option<u64>,
    /// Preferred I/O block size.
    pub block_size: Option<u64>,
    /// Allocated 512-byte blocks.
    pub blocks: Option<u64>,
    /// Last access.
    pub accessed: Option<FileTime>,
    /// Last content modification.
    pub modified: Option<FileTime>,
    /// Last status change.
    pub changed: Option<FileTime>,
    /// Creation (birth) time.
    pub created: Option<FileTime>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// `access(2)` checks. All false tests existence only.
pub struct AccessMode {
    /// Readable by the caller.
    pub read: bool,
    /// Writable by the caller (Windows: not read-only, or a directory).
    pub write: bool,
    /// Executable or searchable by the caller.
    pub execute: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Windows symbolic-link flavour; ignored elsewhere.
pub enum SymlinkKind {
    /// File symbolic link.
    #[default]
    File,
    /// Directory symbolic link.
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Timestamp update for `SetTimes`.
pub enum TimeChange {
    /// Leave this timestamp unchanged.
    Keep,
    /// Set it to the current time.
    Now,
    /// Set it to this time.
    At(FileTime),
}

#[derive(Clone, Debug)]
/// A metadata operation's subject: a path or an open handle.
pub enum FsTarget {
    /// Address by path.
    Path {
        /// Prepared path.
        path: FsPath,
        /// Operate on the link target rather than a final symbolic link.
        follow_symlinks: bool,
    },
    /// Address an open file or directory handle.
    File(Handle),
}

#[derive(Debug)]
/// One typed request, submitted with `Loop::fs`. Operations on the same handle run
/// FIFO. A cancelled mutation that already started is not rolled back.
pub enum FsRequest {
    /// Open a file. Completes with `FsResult::Opened(handle)`; on error or
    /// cancellation no handle becomes visible.
    Open {
        /// Prepared path.
        path: FsPath,
        /// Access and creation options.
        options: FileOptions,
    },
    /// Open a directory stream for `ReadDir`. Completes with `Opened(handle)`.
    OpenDir {
        /// Prepared path.
        path: FsPath,
    },
    /// Close the native descriptor after earlier queued operations, reporting its
    /// error. Later requests fail. Release the handle itself with `Loop::close`,
    /// which otherwise closes a still-open descriptor on the loop thread.
    Close {
        /// Open file or directory.
        file: Handle,
    },
    /// Read once (Node `fs.read`). `offset: None` reads at and advances the
    /// handle's cursor. Zero bytes with a nonempty buffer means end of file.
    Read {
        /// Open file.
        file: Handle,
        /// Provided memory or a pooled lease (pool exhaustion delays the start).
        buffer: ReadBuf,
        /// Positional read offset, or None for the cursor.
        offset: Option<u64>,
    },
    /// Write once (Node `fs.write`); the count may be short. Hosts loop for writeFile.
    Write {
        /// Open file.
        file: Handle,
        /// Stable input bytes.
        buffer: WriteBuf,
        /// Positional write offset, or None for the cursor.
        offset: Option<u64>,
    },
    /// Flush file data, and metadata unless `data_only`.
    Sync {
        /// Open file.
        file: Handle,
        /// `fdatasync` instead of `fsync`.
        data_only: bool,
    },
    /// Set the file length without moving its cursor.
    Truncate {
        /// Open file.
        file: Handle,
        /// New length in bytes.
        size: u64,
    },
    /// Metadata by path (`follow_symlinks: false` is `lstat`).
    Stat {
        /// Prepared path.
        path: FsPath,
        /// Follow a final symbolic link.
        follow_symlinks: bool,
    },
    /// Metadata of an open handle.
    Fstat {
        /// Open file or directory.
        file: Handle,
    },
    /// Read directory entry records ([`DirEntries`]); `.` and `..` are omitted.
    /// A name never spans completions. An empty page with `eof` ends the stream.
    ReadDir {
        /// Directory opened with `OpenDir`.
        dir: Handle,
        /// Record output (ResourceLimit if the next name cannot fit an empty buffer).
        buffer: ReadBuf,
    },
    /// Create one directory; recursive creation belongs to the host.
    Mkdir {
        /// Prepared path.
        path: FsPath,
        /// Permissions filtered by the umask (ignored on Windows and WASI).
        mode: u32,
    },
    /// Remove an empty directory.
    Rmdir {
        /// Prepared path.
        path: FsPath,
    },
    /// Remove a file or symbolic link.
    Unlink {
        /// Prepared path.
        path: FsPath,
    },
    /// Rename, replacing an existing destination where the platform allows.
    Rename {
        /// Existing path.
        from: FsPath,
        /// New path.
        to: FsPath,
    },
    /// Create a hard link.
    Link {
        /// Existing file.
        existing: FsPath,
        /// New link path.
        link: FsPath,
    },
    /// Create a symbolic link whose content is `target`'s original text.
    Symlink {
        /// Link content; not resolved against preopens on WASI.
        target: FsPath,
        /// New link path.
        link: FsPath,
        /// Windows link flavour.
        kind: SymlinkKind,
    },
    /// Read a symbolic link's content into bytes (`FsResult::Bytes`).
    ReadLink {
        /// Prepared path.
        path: FsPath,
        /// Output (ResourceLimit if the content does not fit).
        buffer: ReadBuf,
    },
    /// Resolve an absolute canonical path into bytes (`FsResult::Bytes`).
    RealPath {
        /// Prepared path.
        path: FsPath,
        /// Output (ResourceLimit if the path does not fit).
        buffer: ReadBuf,
    },
    /// Check accessibility (Node `fs.access`).
    Access {
        /// Prepared path.
        path: FsPath,
        /// Checked permissions.
        mode: AccessMode,
    },
    /// Change permissions (Windows: the read-only attribute from the owner write bit).
    Chmod {
        /// Path or handle.
        target: FsTarget,
        /// Permission bits.
        mode: u32,
    },
    /// Change ownership; None leaves an id unchanged.
    Chown {
        /// Path or handle.
        target: FsTarget,
        /// New user id.
        uid: Option<u32>,
        /// New group id.
        gid: Option<u32>,
    },
    /// Change access and modification times.
    SetTimes {
        /// Path or handle.
        target: FsTarget,
        /// Access time update.
        accessed: TimeChange,
        /// Modification time update.
        modified: TimeChange,
    },
    /// Copy file contents (and permissions), failing on an existing destination if exclusive.
    CopyFile {
        /// Source file.
        from: FsPath,
        /// Destination file.
        to: FsPath,
        /// Fail with AlreadyExists when `to` exists.
        exclusive: bool,
    },
}
impl FsRequest {
    /// The handle whose FIFO this request joins, if any.
    pub(crate) fn handle(&self) -> Option<Handle> {
        match self {
            Self::Close { file }
            | Self::Read { file, .. }
            | Self::Write { file, .. }
            | Self::Sync { file, .. }
            | Self::Truncate { file, .. }
            | Self::Fstat { file }
            | Self::ReadDir { dir: file, .. }
            | Self::Chmod {
                target: FsTarget::File(file),
                ..
            }
            | Self::Chown {
                target: FsTarget::File(file),
                ..
            }
            | Self::SetTimes {
                target: FsTarget::File(file),
                ..
            } => Some(*file),
            _ => None,
        }
    }
    /// Whether the request creates a new handle.
    pub(crate) fn opens(&self) -> bool {
        matches!(self, Self::Open { .. } | Self::OpenDir { .. })
    }
    /// The read buffer to fill, if the request produces bytes.
    #[cfg_attr(
        any(turnloop_backend = "web", turnloop_backend = "unsupported"),
        allow(dead_code)
    )]
    pub(crate) fn read_buffer(&mut self) -> Option<&mut ReadBuf> {
        match self {
            Self::Read { buffer, .. }
            | Self::ReadDir { buffer, .. }
            | Self::ReadLink { buffer, .. }
            | Self::RealPath { buffer, .. } => Some(buffer),
            _ => None,
        }
    }
}

#[derive(Debug)]
/// Successful typed results. Failures use `OpResult::Err`.
pub enum FsResult {
    /// A new, visible file or directory handle; release it with `Loop::close`.
    Opened(Handle),
    /// Bytes read; zero with a nonempty buffer is end of file.
    Read {
        /// Bytes initialized in the provided buffer or lease.
        n: usize,
        /// The pooled lease, when `ReadBuf::Pooled` was requested.
        lease: Option<BufLease>,
    },
    /// Bytes written; may be short.
    Wrote(usize),
    /// File metadata, in reusable loop storage (keeps completions small).
    Metadata(Metadata),
    /// Directory entry records ([`DirEntries`]).
    Directory {
        /// Initialized record bytes.
        n: usize,
        /// The pooled lease, when requested.
        lease: Option<BufLease>,
        /// No further entries remain.
        eof: bool,
    },
    /// Link content or a canonical path (native bytes on Unix, WTF-8 on Windows, UTF-8 on WASI).
    Bytes {
        /// Initialized bytes.
        n: usize,
        /// The pooled lease, when requested.
        lease: Option<BufLease>,
    },
    /// The operation succeeded without a value.
    Done,
}

/// Metadata result held in reusable per-loop storage; dereferences to
/// [`FileMetadata`]. Dropping it returns the storage, so steady-state `stat`
/// requests allocate nothing once the loop has served one. Copy the value out
/// to keep it beyond the completion.
// The pool holds boxes so a lease can move one out and dereference it stably.
#[allow(clippy::vec_box)]
pub struct Metadata {
    value: Option<Box<FileMetadata>>,
    pool: std::rc::Rc<std::cell::RefCell<Vec<Box<FileMetadata>>>>,
}
impl std::ops::Deref for Metadata {
    type Target = FileMetadata;
    fn deref(&self) -> &FileMetadata {
        self.value.as_ref().expect("live metadata")
    }
}
impl std::fmt::Debug for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}
impl Drop for Metadata {
    fn drop(&mut self) {
        let mut pool = self.pool.borrow_mut();
        if let Some(value) = self.value.take()
            && pool.len() < pool.capacity()
        {
            pool.push(value);
        }
    }
}
/// Bounded reusable metadata storage, one per loop.
#[derive(Clone)]
#[allow(clippy::vec_box)]
pub(crate) struct MetadataPool(std::rc::Rc<std::cell::RefCell<Vec<Box<FileMetadata>>>>);
impl MetadataPool {
    pub fn new(capacity: usize) -> Self {
        Self(std::rc::Rc::new(std::cell::RefCell::new(
            Vec::with_capacity(capacity),
        )))
    }
    pub fn lease(&self, metadata: FileMetadata) -> Metadata {
        let value = match self.0.borrow_mut().pop() {
            Some(mut boxed) => {
                *boxed = metadata;
                boxed
            }
            None => Box::new(metadata),
        };
        Metadata {
            value: Some(value),
            pool: self.0.clone(),
        }
    }
}

/// Backend- or worker-side typed result; the core attaches leases and handles.
/// Metadata is inline: results travel through fixed, preallocated event storage,
/// and boxing would allocate per `stat`. The pool path keeps metadata in its slot.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum FsOutput {
    /// The request's new handle is bound.
    Opened,
    /// Bytes read.
    Read(usize),
    /// Bytes written.
    Wrote(usize),
    /// Metadata.
    Metadata(FileMetadata),
    /// Directory record bytes.
    Directory {
        /// Initialized record bytes.
        n: usize,
        /// No further entries remain.
        eof: bool,
    },
    /// Result bytes (link content or canonical path).
    Bytes(usize),
    /// Success without a value.
    Done,
}

/// Size of a record header: tag, reserved zero, little-endian u16 name length.
pub const RECORD_HEADER: usize = 4;
/// Append one record; false when it does not fit the remaining space.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn put_record(output: &mut [u8], used: &mut usize, tag: u8, name: &[u8]) -> bool {
    let Ok(len) = u16::try_from(name.len()) else {
        return false;
    };
    let end = *used + RECORD_HEADER + name.len();
    if end > output.len() {
        return false;
    }
    output[*used] = tag;
    output[*used + 1] = 0;
    output[*used + 2..*used + 4].copy_from_slice(&len.to_le_bytes());
    output[*used + RECORD_HEADER..end].copy_from_slice(name);
    *used = end;
    true
}
#[derive(Clone, Debug)]
struct Records<'a>(&'a [u8]);
impl<'a> Iterator for Records<'a> {
    type Item = (u8, &'a [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        if self.0.len() < RECORD_HEADER {
            return None;
        }
        let len = u16::from_le_bytes([self.0[2], self.0[3]]) as usize;
        let end = (RECORD_HEADER + len).min(self.0.len());
        let record = (self.0[0], &self.0[RECORD_HEADER..end]);
        self.0 = &self.0[end..];
        Some(record)
    }
}

#[derive(Clone, Debug)]
/// Iterate `(type, name)` directory records from a `FsResult::Directory` page.
/// Names are native bytes on Unix, WTF-8 on Windows and UTF-8 on WASI.
pub struct DirEntries<'a>(Records<'a>);
impl<'a> DirEntries<'a> {
    /// Parse the initialized bytes of one page.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self(Records(bytes))
    }
}
impl<'a> Iterator for DirEntries<'a> {
    type Item = (FileType, &'a [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|(tag, name)| (FileType::from_tag(tag), name))
    }
}

#[derive(Clone, Copy, Debug, Default)]
/// Filesystem watch options.
pub struct WatchOptions {
    /// Include all descendants. Supported by FSEvents (macOS) and
    /// ReadDirectoryChangesW (Windows); inotify and kqueue return Unsupported,
    /// and hosts emulate recursion with one watch per directory, as Node does on Linux.
    pub recursive: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Node `fs.watch` event type.
pub enum WatchKind {
    /// An entry appeared, disappeared or was renamed (or the watched root itself).
    Rename = 1,
    /// Contents or metadata changed.
    Change = 2,
}

#[derive(Clone, Debug)]
/// Iterate `(kind, name)` records of an `OpResult::Watch` batch, in native order.
/// Names are relative to the watched directory using native separators; an event
/// on the watched root (or a watched file) carries the root's final component.
pub struct WatchEvents<'a>(Records<'a>);
impl<'a> WatchEvents<'a> {
    /// Parse the initialized bytes of one batch.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self(Records(bytes))
    }
}
impl<'a> Iterator for WatchEvents<'a> {
    type Item = (WatchKind, &'a [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(tag, name)| {
            (
                if tag == WatchKind::Change as u8 {
                    WatchKind::Change
                } else {
                    WatchKind::Rename
                },
                name,
            )
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod service;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use service::{Reply, Service};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod watch;

/// Encode UTF-16 as WTF-8 (lossless for unpaired surrogates), appending to `out`.
#[cfg(windows)]
pub(crate) fn wtf8(units: &[u16], out: &mut [u8], used: &mut usize) -> bool {
    let start = *used;
    for c in char::decode_utf16(units.iter().copied()) {
        let code = match c {
            Ok(c) => c as u32,
            Err(e) => u32::from(e.unpaired_surrogate()),
        };
        let mut bytes = [0u8; 4];
        let len = if code < 0x80 {
            bytes[0] = code as u8;
            1
        } else if code < 0x800 {
            bytes[0] = 0xc0 | (code >> 6) as u8;
            bytes[1] = 0x80 | (code & 0x3f) as u8;
            2
        } else if code < 0x10000 {
            bytes[0] = 0xe0 | (code >> 12) as u8;
            bytes[1] = 0x80 | ((code >> 6) & 0x3f) as u8;
            bytes[2] = 0x80 | (code & 0x3f) as u8;
            3
        } else {
            bytes[0] = 0xf0 | (code >> 18) as u8;
            bytes[1] = 0x80 | ((code >> 12) & 0x3f) as u8;
            bytes[2] = 0x80 | ((code >> 6) & 0x3f) as u8;
            bytes[3] = 0x80 | (code & 0x3f) as u8;
            4
        };
        if *used + len > out.len() {
            *used = start;
            return false;
        }
        out[*used..*used + len].copy_from_slice(&bytes[..len]);
        *used += len;
    }
    true
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    #[test]
    fn records_round_trip_and_truncated_input_stops() {
        let mut page = [0u8; 32];
        let mut used = 0;
        assert!(put_record(&mut page, &mut used, 2, b"dir"));
        assert!(put_record(&mut page, &mut used, 1, b""));
        assert!(!put_record(&mut page, &mut used, 1, &[b'x'; 30]));
        let entries: Vec<_> = DirEntries::new(&page[..used]).collect();
        assert_eq!(
            entries,
            [
                (FileType::Directory, &b"dir"[..]),
                (FileType::File, &b""[..])
            ]
        );
        let events: Vec<_> = WatchEvents::new(&page[..used]).collect();
        assert_eq!(events[0].0, WatchKind::Change);
        assert_eq!(events[1].0, WatchKind::Rename);
        assert_eq!(DirEntries::new(&page[..3]).count(), 0);
    }
    #[cfg(windows)]
    #[test]
    fn wtf8_preserves_unpaired_surrogates() {
        let mut out = [0u8; 16];
        let mut used = 0;
        assert!(wtf8(&[0x61, 0xd800, 0x20ac], &mut out, &mut used));
        assert_eq!(&out[..used], b"a\xed\xa0\x80\xe2\x82\xac");
    }
}
