//! `wasi:filesystem@0.2` binding surface for the shared WASI filesystem core.
//! Reads lower directly into caller memory; writes, metadata and path calls lower
//! borrowed slices and strings, so steady-state requests allocate nothing.
use super::super::wasi_fs::{Api, errno, error};
use crate::{
    Error, ErrorKind, Result,
    fs::{FileMetadata, FileOptions, FileTime, FileType, TimeChange},
};
use wasip2::{
    clocks::wall_clock::Datetime,
    filesystem::types::{
        Descriptor, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntryStream,
        ErrorCode, NewTimestamp, OpenFlags, PathFlags,
    },
    io::streams::OutputStream,
};

pub(in crate::backend) struct P2;

fn errno_of(code: ErrorCode) -> i32 {
    use ErrorCode as E;
    match code {
        E::Access => 2,
        E::WouldBlock => 6,
        E::Already => 7,
        E::BadDescriptor => 8,
        E::Busy => 10,
        E::Deadlock => 16,
        E::Quota => 19,
        E::Exist => 20,
        E::FileTooLarge => 22,
        E::IllegalByteSequence => 25,
        E::InProgress => 26,
        E::Interrupted => 27,
        E::Invalid => 28,
        E::Io => 29,
        E::IsDirectory => 31,
        E::Loop => 32,
        E::TooManyLinks => 34,
        E::MessageSize => 35,
        E::NameTooLong => 37,
        E::NoDevice => 43,
        E::NoEntry => 44,
        E::NoLock => 46,
        E::InsufficientMemory => 48,
        E::InsufficientSpace => 51,
        E::NotDirectory => 54,
        E::NotEmpty => 55,
        E::NotRecoverable => 56,
        E::Unsupported => 58,
        E::NoTty => 59,
        E::NoSuchDevice => 60,
        E::Overflow => 61,
        E::NotPermitted => 63,
        E::Pipe => 64,
        E::ReadOnly => 69,
        E::InvalidSeek => 70,
        E::TextFileBusy => 74,
        E::CrossDevice => 75,
    }
}
fn fail(code: ErrorCode) -> Error {
    error(errno_of(code))
}
fn follow(yes: bool) -> PathFlags {
    if yes {
        PathFlags::SYMLINK_FOLLOW
    } else {
        PathFlags::empty()
    }
}
fn kind(t: DescriptorType) -> FileType {
    match t {
        DescriptorType::RegularFile => FileType::File,
        DescriptorType::Directory => FileType::Directory,
        DescriptorType::SymbolicLink => FileType::Symlink,
        DescriptorType::BlockDevice => FileType::BlockDevice,
        DescriptorType::CharacterDevice => FileType::CharDevice,
        DescriptorType::Fifo => FileType::Fifo,
        DescriptorType::Socket => FileType::Socket,
        DescriptorType::Unknown => FileType::Unknown,
    }
}
fn time(t: Datetime) -> FileTime {
    FileTime {
        seconds: t.seconds as i64,
        nanoseconds: t.nanoseconds,
    }
}
fn metadata(s: DescriptorStat) -> FileMetadata {
    FileMetadata {
        kind: kind(s.type_),
        size: s.size,
        mode: None,
        device: None,
        inode: None,
        links: Some(s.link_count),
        uid: None,
        gid: None,
        rdev: None,
        block_size: None,
        blocks: None,
        accessed: s.data_access_timestamp.map(time),
        modified: s.data_modification_timestamp.map(time),
        changed: s.status_change_timestamp.map(time),
        created: None,
    }
}
fn timestamp(change: TimeChange) -> Result<NewTimestamp> {
    Ok(match change {
        TimeChange::Keep => NewTimestamp::NoChange,
        TimeChange::Now => NewTimestamp::Now,
        TimeChange::At(t) => NewTimestamp::Timestamp(Datetime {
            seconds: u64::try_from(t.seconds).map_err(|_| error(errno::EINVAL))?,
            nanoseconds: t.nanoseconds,
        }),
    })
}

impl Api for P2 {
    type Descriptor = Descriptor;
    type Appender = OutputStream;
    type Entries = DirectoryEntryStream;
    fn preopens() -> Vec<(Descriptor, String)> {
        wasip2::filesystem::preopens::get_directories()
    }
    fn open_at(
        dir: &Descriptor,
        path: &str,
        o: &FileOptions,
        directory: bool,
    ) -> Result<Descriptor> {
        let mut open = OpenFlags::empty();
        if o.create || o.exclusive {
            open |= OpenFlags::CREATE;
        }
        if o.exclusive {
            open |= OpenFlags::EXCLUSIVE;
        }
        if o.truncate {
            open |= OpenFlags::TRUNCATE;
        }
        if directory {
            open |= OpenFlags::DIRECTORY;
        }
        let mut flags = DescriptorFlags::empty();
        if o.read {
            flags |= DescriptorFlags::READ;
        }
        if o.write || o.append {
            flags |= DescriptorFlags::WRITE;
        }
        if o.sync {
            flags |= DescriptorFlags::FILE_INTEGRITY_SYNC;
        }
        if o.data_sync {
            flags |= DescriptorFlags::DATA_INTEGRITY_SYNC;
        }
        if flags.is_empty() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        dir.open_at(follow(o.follow_symlinks), path, open, flags)
            .map_err(fail)
    }
    fn read(file: &Descriptor, output: &mut [u8], offset: u64) -> Result<(usize, bool)> {
        super::abi::file_read(file, output, offset).map_err(|code| {
            // SAFETY: the host returned a valid error-code discriminant.
            fail(unsafe { ErrorCode::_lift(code) })
        })
    }
    fn write(file: &Descriptor, bytes: &[u8], offset: u64) -> Result<usize> {
        file.write(bytes, offset).map(|n| n as usize).map_err(fail)
    }
    fn append(
        file: &Descriptor,
        appender: &mut Option<OutputStream>,
        bytes: &[u8],
    ) -> Result<usize> {
        if appender.is_none() {
            *appender = Some(file.append_via_stream().map_err(fail)?);
        }
        let stream = appender.as_ref().expect("append stream");
        // blocking-write-and-flush accepts at most 4096 bytes; report a short write.
        let n = bytes.len().min(4096);
        stream
            .blocking_write_and_flush(&bytes[..n])
            .map_err(|_| error(errno::EBADF))?;
        Ok(n)
    }
    fn stat(file: &Descriptor) -> Result<FileMetadata> {
        file.stat().map(metadata).map_err(fail)
    }
    fn stat_at(dir: &Descriptor, path: &str, yes: bool) -> Result<FileMetadata> {
        dir.stat_at(follow(yes), path).map(metadata).map_err(fail)
    }
    fn sync(file: &Descriptor, data_only: bool) -> Result<()> {
        if data_only {
            file.sync_data()
        } else {
            file.sync()
        }
        .map_err(fail)
    }
    fn set_size(file: &Descriptor, size: u64) -> Result<()> {
        file.set_size(size).map_err(fail)
    }
    fn set_times(file: &Descriptor, accessed: TimeChange, modified: TimeChange) -> Result<()> {
        file.set_times(timestamp(accessed)?, timestamp(modified)?)
            .map_err(fail)
    }
    fn set_times_at(
        dir: &Descriptor,
        path: &str,
        yes: bool,
        accessed: TimeChange,
        modified: TimeChange,
    ) -> Result<()> {
        dir.set_times_at(
            follow(yes),
            path,
            timestamp(accessed)?,
            timestamp(modified)?,
        )
        .map_err(fail)
    }
    fn entries(dir: &Descriptor) -> Result<DirectoryEntryStream> {
        dir.read_directory().map_err(fail)
    }
    fn next_entry(entries: &mut DirectoryEntryStream) -> Result<Option<(FileType, String)>> {
        entries
            .read_directory_entry()
            .map(|entry| entry.map(|e| (kind(e.type_), e.name)))
            .map_err(fail)
    }
    fn create_directory_at(dir: &Descriptor, path: &str) -> Result<()> {
        dir.create_directory_at(path).map_err(fail)
    }
    fn remove_directory_at(dir: &Descriptor, path: &str) -> Result<()> {
        dir.remove_directory_at(path).map_err(fail)
    }
    fn unlink_file_at(dir: &Descriptor, path: &str) -> Result<()> {
        dir.unlink_file_at(path).map_err(fail)
    }
    fn rename_at(dir: &Descriptor, path: &str, new_dir: &Descriptor, new_path: &str) -> Result<()> {
        dir.rename_at(path, new_dir, new_path).map_err(fail)
    }
    fn link_at(dir: &Descriptor, path: &str, new_dir: &Descriptor, new_path: &str) -> Result<()> {
        dir.link_at(PathFlags::empty(), path, new_dir, new_path)
            .map_err(fail)
    }
    fn symlink_at(dir: &Descriptor, target: &str, path: &str) -> Result<()> {
        dir.symlink_at(target, path).map_err(fail)
    }
    fn readlink_at(dir: &Descriptor, path: &str) -> Result<String> {
        dir.readlink_at(path).map_err(fail)
    }
}
