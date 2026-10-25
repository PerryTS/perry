use super::{
    bindings::wasi::{
        clocks::wall_clock::Datetime,
        filesystem::{preopens, types::*},
    },
    errors::{FsError, FsResult, IoError},
    State,
};
use cap_fs_ext::{
    DirExt, FileTypeExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsMaybeDirExt,
};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use system_interface::fs::FileIoExt;
use wasmtime::component::Resource;

pub enum Object {
    File(Arc<File>),
    Directory(Arc<Dir>),
}
pub struct Descriptor {
    pub object: Object,
    pub flags: DescriptorFlags,
    // This is the authority attached to the open descriptor, including the
    // inherited ceiling on capabilities of subsequently opened children.
    pub read_only: bool,
}
pub struct DirectoryEntries(pub cap_std::fs::ReadDir);
impl Descriptor {
    pub fn file(&self) -> FsResult<&Arc<File>> {
        match &self.object {
            Object::File(f) => Ok(f),
            _ => Err(ErrorCode::BadDescriptor.into()),
        }
    }
    pub fn dir(&self) -> FsResult<&Dir> {
        match &self.object {
            Object::Directory(d) => Ok(d),
            _ => Err(ErrorCode::NotDirectory.into()),
        }
    }
    pub fn writable(&self) -> FsResult<()> {
        if self.read_only {
            Err(ErrorCode::NotPermitted.into())
        } else {
            Ok(())
        }
    }
    pub fn file_read(&self) -> FsResult<&Arc<File>> {
        self.file()
    }
    pub fn file_write(&self) -> FsResult<&Arc<File>> {
        let file = self.file()?;
        self.writable()?;
        Ok(file)
    }
    fn mutate(&self) -> FsResult<&Dir> {
        let dir = self.dir()?;
        self.writable()?;
        Ok(dir)
    }
    fn metadata(&self) -> FsResult<Metadata> {
        Ok(match &self.object {
            Object::File(f) => f.metadata()?,
            Object::Directory(d) => d.dir_metadata()?,
        })
    }
    fn metadata_at(&self, flags: PathFlags, path: &str) -> FsResult<Metadata> {
        let d = self.dir()?;
        Ok(if flags.contains(PathFlags::SYMLINK_FOLLOW) {
            d.metadata(path)?
        } else {
            d.symlink_metadata(path)?
        })
    }
}

pub fn open_preopens(
    options: &crate::WasiOptions,
) -> wasmtime::Result<Vec<(Arc<Dir>, String, bool)>> {
    options
        .preopens
        .iter()
        .map(|p| {
            let dir = Dir::open_ambient_dir(&p.host_path, cap_std::ambient_authority())?;
            Ok((Arc::new(dir), p.guest_path.clone(), p.read_only))
        })
        .collect()
}

impl preopens::Host for State {
    fn get_directories(&mut self) -> wasmtime::Result<Vec<(Resource<Descriptor>, String)>> {
        let mut result = Vec::new();
        for (dir, name, read_only) in &self.preopens {
            let flags = DescriptorFlags::READ
                | if *read_only {
                    DescriptorFlags::empty()
                } else {
                    DescriptorFlags::MUTATE_DIRECTORY
                };
            let descriptor = Descriptor {
                object: Object::Directory(dir.clone()),
                flags,
                read_only: *read_only,
            };
            result.push((self.table.push(descriptor)?, name.clone()));
        }
        Ok(result)
    }
}
impl super::bindings::wasi::filesystem::types::Host for State {
    fn convert_error_code(&mut self, error: FsError) -> wasmtime::Result<ErrorCode> {
        match error {
            FsError::Code(code) => Ok(code),
            FsError::Trap(error) => Err(error),
        }
    }
    fn filesystem_error_code(
        &mut self,
        error: Resource<IoError>,
    ) -> wasmtime::Result<Option<ErrorCode>> {
        Ok(Some(super::errors::code(&self.table.get(&error)?.0)))
    }
}

fn file_type(t: cap_std::fs::FileType) -> DescriptorType {
    if t.is_dir() {
        DescriptorType::Directory
    } else if t.is_file() {
        DescriptorType::RegularFile
    } else if t.is_symlink() {
        DescriptorType::SymbolicLink
    } else if t.is_block_device() {
        DescriptorType::BlockDevice
    } else if t.is_char_device() {
        DescriptorType::CharacterDevice
    } else {
        DescriptorType::Unknown
    }
}
fn datetime(time: SystemTime) -> Option<Datetime> {
    time.duration_since(UNIX_EPOCH).ok().map(|d| Datetime {
        seconds: d.as_secs(),
        nanoseconds: d.subsec_nanos(),
    })
}
fn stat(metadata: Metadata) -> DescriptorStat {
    DescriptorStat {
        type_: file_type(metadata.file_type()),
        link_count: metadata.nlink(),
        size: metadata.len(),
        data_access_timestamp: metadata
            .accessed()
            .ok()
            .and_then(|t| datetime(t.into_std())),
        data_modification_timestamp: metadata
            .modified()
            .ok()
            .and_then(|t| datetime(t.into_std())),
        status_change_timestamp: metadata.created().ok().and_then(|t| datetime(t.into_std())),
    }
}

fn timestamp(t: NewTimestamp) -> FsResult<Option<cap_fs_ext::SystemTimeSpec>> {
    use cap_fs_ext::SystemTimeSpec;
    Ok(match t {
        NewTimestamp::NoChange => None,
        NewTimestamp::Now => Some(SystemTimeSpec::SymbolicNow),
        NewTimestamp::Timestamp(t) => {
            let seconds = t
                .seconds
                .checked_add(u64::from(t.nanoseconds / 1_000_000_000))
                .ok_or(ErrorCode::Overflow)?;
            let t = UNIX_EPOCH
                .checked_add(Duration::new(seconds, t.nanoseconds % 1_000_000_000))
                .ok_or(ErrorCode::Overflow)?;
            Some(SystemTimeSpec::Absolute(
                cap_std::time::SystemTime::from_std(t),
            ))
        }
    })
}
fn standard_timestamp(
    t: Option<cap_fs_ext::SystemTimeSpec>,
) -> Option<fs_set_times::SystemTimeSpec> {
    t.map(|t| match t {
        cap_fs_ext::SystemTimeSpec::SymbolicNow => fs_set_times::SystemTimeSpec::SymbolicNow,
        cap_fs_ext::SystemTimeSpec::Absolute(t) => {
            fs_set_times::SystemTimeSpec::Absolute(t.into_std())
        }
    })
}
fn sync_file_result(result: std::io::Result<()>) -> FsResult<()> {
    #[cfg(windows)]
    if result.as_ref().is_err_and(|error| {
        error.raw_os_error() == Some(windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED as i32)
    }) {
        return Ok(());
    }
    result.map_err(Into::into)
}

fn hash(metadata: Metadata) -> MetadataHashValue {
    MetadataHashValue {
        lower: metadata.ino(),
        upper: metadata.dev(),
    }
}

impl HostDescriptor for State {
    fn drop(&mut self, fd: Resource<Descriptor>) -> wasmtime::Result<()> {
        self.table.delete(fd)?;
        Ok(())
    }
    fn read_via_stream(
        &mut self,
        fd: Resource<Descriptor>,
        offset: u64,
    ) -> FsResult<Resource<super::streams::Input>> {
        let descriptor = self.table.get(&fd)?;
        if matches!(descriptor.object, Object::Directory(_)) {
            return Err(ErrorCode::IsDirectory.into());
        }
        let file = descriptor.file_read()?.clone();
        Ok(self.table.push(super::streams::Input::file(file, offset))?)
    }
    fn write_via_stream(
        &mut self,
        fd: Resource<Descriptor>,
        offset: u64,
    ) -> FsResult<Resource<super::streams::Output>> {
        let file = self.table.get(&fd)?.file_write()?.clone();
        Ok(self
            .table
            .push(super::streams::Output::file(file, Some(offset)))?)
    }
    fn append_via_stream(
        &mut self,
        fd: Resource<Descriptor>,
    ) -> FsResult<Resource<super::streams::Output>> {
        let file = self.table.get(&fd)?.file_write()?.clone();
        Ok(self.table.push(super::streams::Output::file(file, None))?)
    }
    fn advise(
        &mut self,
        fd: Resource<Descriptor>,
        offset: u64,
        length: u64,
        advice: Advice,
    ) -> FsResult<()> {
        use system_interface::fs::Advice as A;
        let advice = match advice {
            Advice::Normal => A::Normal,
            Advice::Sequential => A::Sequential,
            Advice::Random => A::Random,
            Advice::WillNeed => A::WillNeed,
            Advice::DontNeed => A::DontNeed,
            Advice::NoReuse => A::NoReuse,
        };
        self.table
            .get(&fd)?
            .file()?
            .advise(offset, length, advice)?;
        Ok(())
    }
    fn sync_data(&mut self, fd: Resource<Descriptor>) -> FsResult<()> {
        match &self.table.get(&fd)?.object {
            Object::File(f) => sync_file_result(f.sync_data())?,
            Object::Directory(d) => d.open(".")?.sync_data()?,
        }
        Ok(())
    }
    fn sync(&mut self, fd: Resource<Descriptor>) -> FsResult<()> {
        match &self.table.get(&fd)?.object {
            Object::File(f) => sync_file_result(f.sync_all())?,
            Object::Directory(d) => d.open(".")?.sync_all()?,
        }
        Ok(())
    }
    fn get_flags(&mut self, fd: Resource<Descriptor>) -> FsResult<DescriptorFlags> {
        Ok(self.table.get(&fd)?.flags)
    }
    fn get_type(&mut self, fd: Resource<Descriptor>) -> FsResult<DescriptorType> {
        Ok(file_type(self.table.get(&fd)?.metadata()?.file_type()))
    }
    fn set_size(&mut self, fd: Resource<Descriptor>, size: u64) -> FsResult<()> {
        self.table.get(&fd)?.file_write()?.set_len(size)?;
        Ok(())
    }
    fn set_times(
        &mut self,
        fd: Resource<Descriptor>,
        access: NewTimestamp,
        modification: NewTimestamp,
    ) -> FsResult<()> {
        use fs_set_times::SetTimes;
        let d = self.table.get(&fd)?;
        d.writable()?;
        let a = timestamp(access)?;
        let m = timestamp(modification)?;
        match &d.object {
            Object::File(f) => SetTimes::set_times(
                &f.try_clone()?.into_std(),
                standard_timestamp(a),
                standard_timestamp(m),
            )?,
            Object::Directory(d) => DirExt::set_times(d.as_ref(), ".", a, m)?,
        }
        Ok(())
    }
    fn read(
        &mut self,
        fd: Resource<Descriptor>,
        length: u64,
        offset: u64,
    ) -> FsResult<(Vec<u8>, bool)> {
        let f = self.table.get(&fd)?.file_read()?;
        let mut bytes = vec![0; length.min(64 * 1024) as usize];
        let n = f.read_at(&mut bytes, offset)?;
        bytes.truncate(n);
        Ok((bytes, n == 0))
    }
    fn write(&mut self, fd: Resource<Descriptor>, bytes: Vec<u8>, offset: u64) -> FsResult<u64> {
        Ok(self
            .table
            .get(&fd)?
            .file_write()?
            .write_at(&bytes, offset)? as u64)
    }
    fn read_directory(&mut self, fd: Resource<Descriptor>) -> FsResult<Resource<DirectoryEntries>> {
        let entries = self.table.get(&fd)?.dir()?.entries()?;
        Ok(self.table.push(DirectoryEntries(entries))?)
    }
    fn create_directory_at(&mut self, fd: Resource<Descriptor>, path: String) -> FsResult<()> {
        self.table.get(&fd)?.mutate()?.create_dir(path)?;
        Ok(())
    }
    fn stat(&mut self, fd: Resource<Descriptor>) -> FsResult<DescriptorStat> {
        Ok(stat(self.table.get(&fd)?.metadata()?))
    }
    fn stat_at(
        &mut self,
        fd: Resource<Descriptor>,
        flags: PathFlags,
        path: String,
    ) -> FsResult<DescriptorStat> {
        Ok(stat(self.table.get(&fd)?.metadata_at(flags, &path)?))
    }
    fn set_times_at(
        &mut self,
        fd: Resource<Descriptor>,
        flags: PathFlags,
        path: String,
        access: NewTimestamp,
        modification: NewTimestamp,
    ) -> FsResult<()> {
        let dir = self.table.get(&fd)?.mutate()?;
        let (a, m) = (timestamp(access)?, timestamp(modification)?);
        if flags.contains(PathFlags::SYMLINK_FOLLOW) {
            dir.set_times(path, a, m)?;
        } else {
            dir.set_symlink_times(path, a, m)?;
        }
        Ok(())
    }
    fn link_at(
        &mut self,
        fd: Resource<Descriptor>,
        flags: PathFlags,
        old: String,
        target: Resource<Descriptor>,
        new: String,
    ) -> FsResult<()> {
        // cap-std's link is descriptor-relative and does not follow the final
        // symlink. WASI explicitly permits rejecting the follow case.
        if flags.contains(PathFlags::SYMLINK_FOLLOW) {
            return Err(ErrorCode::Invalid.into());
        }
        self.table
            .get(&fd)?
            .mutate()?
            .hard_link(old, self.table.get(&target)?.mutate()?, new)?;
        Ok(())
    }
    fn open_at(
        &mut self,
        fd: Resource<Descriptor>,
        paths: PathFlags,
        path: String,
        open: OpenFlags,
        flags: DescriptorFlags,
    ) -> FsResult<Resource<Descriptor>> {
        let parent = self.table.get(&fd)?;
        let dir = parent.dir()?;
        if flags.intersects(
            DescriptorFlags::FILE_INTEGRITY_SYNC
                | DescriptorFlags::DATA_INTEGRITY_SYNC
                | DescriptorFlags::REQUESTED_WRITE_SYNC,
        ) {
            return Err(ErrorCode::Unsupported.into());
        }
        if open.contains(OpenFlags::DIRECTORY)
            && open.intersects(OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::EXCLUSIVE)
        {
            return Err(ErrorCode::Invalid.into());
        }
        // Record the actual open mode, including the former provider's implicit
        // read mode and the write mode required by create/truncate.
        let read = flags.contains(DescriptorFlags::READ) || !flags.contains(DescriptorFlags::WRITE);
        let write = flags.contains(DescriptorFlags::WRITE)
            || open.intersects(OpenFlags::CREATE | OpenFlags::TRUNCATE);
        if write {
            parent.writable()?;
        }
        let read_only = parent.read_only;
        let mut options = OpenOptions::new();
        options
            .read(read)
            .write(write)
            .create(open.contains(OpenFlags::CREATE) && !open.contains(OpenFlags::EXCLUSIVE))
            .create_new(open.contains(OpenFlags::CREATE) && open.contains(OpenFlags::EXCLUSIVE))
            .truncate(open.contains(OpenFlags::TRUNCATE))
            .follow(if paths.contains(PathFlags::SYMLINK_FOLLOW) {
                FollowSymlinks::Yes
            } else {
                FollowSymlinks::No
            })
            .maybe_dir(true);
        let file = dir.open_with(&path, &options)?;
        let directory = file.metadata()?.is_dir();
        if open.contains(OpenFlags::DIRECTORY) && !directory {
            return Err(ErrorCode::NotDirectory.into());
        }
        if directory && flags.contains(DescriptorFlags::WRITE) {
            return Err(ErrorCode::IsDirectory.into());
        }
        let mut mode = DescriptorFlags::empty();
        if read {
            mode |= DescriptorFlags::READ;
        }
        if write {
            mode |= if directory {
                DescriptorFlags::MUTATE_DIRECTORY
            } else {
                DescriptorFlags::WRITE
            };
        }
        let object = if directory {
            Object::Directory(Arc::new(Dir::from_std_file(file.into_std())))
        } else {
            Object::File(Arc::new(file))
        };
        Ok(self.table.push(Descriptor {
            object,
            flags: mode,
            read_only,
        })?)
    }

    fn readlink_at(&mut self, fd: Resource<Descriptor>, path: String) -> FsResult<String> {
        self.table
            .get(&fd)?
            .dir()?
            .read_link_contents(path)?
            .into_os_string()
            .into_string()
            .map_err(|_| ErrorCode::IllegalByteSequence.into())
    }
    fn remove_directory_at(&mut self, fd: Resource<Descriptor>, path: String) -> FsResult<()> {
        self.table.get(&fd)?.mutate()?.remove_dir(path)?;
        Ok(())
    }
    fn rename_at(
        &mut self,
        fd: Resource<Descriptor>,
        old: String,
        target: Resource<Descriptor>,
        new: String,
    ) -> FsResult<()> {
        self.table
            .get(&fd)?
            .mutate()?
            .rename(old, self.table.get(&target)?.mutate()?, new)?;
        Ok(())
    }
    fn symlink_at(&mut self, fd: Resource<Descriptor>, old: String, new: String) -> FsResult<()> {
        self.table.get(&fd)?.mutate()?.symlink(old, new)?;
        Ok(())
    }
    fn unlink_file_at(&mut self, fd: Resource<Descriptor>, path: String) -> FsResult<()> {
        self.table
            .get(&fd)?
            .mutate()?
            .remove_file_or_symlink(path)?;
        Ok(())
    }
    fn is_same_object(
        &mut self,
        fd: Resource<Descriptor>,
        other: Resource<Descriptor>,
    ) -> wasmtime::Result<bool> {
        let a = self
            .table
            .get(&fd)?
            .metadata()
            .map_err(|e| wasmtime::format_err!("{e}"))?;
        let b = self
            .table
            .get(&other)?
            .metadata()
            .map_err(|e| wasmtime::format_err!("{e}"))?;
        Ok(a.dev() == b.dev() && a.ino() == b.ino())
    }
    fn metadata_hash(&mut self, fd: Resource<Descriptor>) -> FsResult<MetadataHashValue> {
        Ok(hash(self.table.get(&fd)?.metadata()?))
    }
    fn metadata_hash_at(
        &mut self,
        fd: Resource<Descriptor>,
        flags: PathFlags,
        path: String,
    ) -> FsResult<MetadataHashValue> {
        Ok(hash(self.table.get(&fd)?.metadata_at(flags, &path)?))
    }
}
impl HostDirectoryEntryStream for State {
    fn drop(&mut self, stream: Resource<DirectoryEntries>) -> wasmtime::Result<()> {
        self.table.delete(stream)?;
        Ok(())
    }
    fn read_directory_entry(
        &mut self,
        stream: Resource<DirectoryEntries>,
    ) -> FsResult<Option<DirectoryEntry>> {
        self.table
            .get_mut(&stream)?
            .0
            .next()
            .map(|entry| {
                let entry = entry?;
                Ok(DirectoryEntry {
                    type_: file_type(entry.file_type()?),
                    name: entry
                        .file_name()
                        .into_string()
                        .map_err(|_| ErrorCode::IllegalByteSequence)?,
                })
            })
            .transpose()
    }
}
#[cfg(test)]
mod tests {
    use super::super::{
        bindings::wasi::io::streams::{HostInputStream, HostOutputStream},
        streams::{Input, Output},
    };
    use super::*;
    use crate::{Preopen, WasiOptions};
    use std::path::PathBuf;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let mut random = [0; 8];
            getrandom::fill(&mut random).unwrap();
            let p = std::env::temp_dir().join(format!(
                "WSA-filesystem-{:016x}",
                u64::from_le_bytes(random)
            ));
            std::fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn state(&self, read_only: bool) -> State {
            State::new(&WasiOptions {
                preopens: vec![Preopen {
                    host_path: self.0.to_string_lossy().into_owned(),
                    guest_path: "/data".into(),
                    read_only,
                }],
                ..Default::default()
            })
            .unwrap()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn borrow<T: 'static>(resource: &Resource<T>) -> Resource<T> {
        Resource::new_borrow(resource.rep())
    }
    fn root(state: &mut State) -> Resource<Descriptor> {
        preopens::Host::get_directories(state).unwrap().remove(0).0
    }
    fn open(
        state: &mut State,
        dir: &Resource<Descriptor>,
        path: &str,
        flags: DescriptorFlags,
    ) -> Resource<Descriptor> {
        state
            .open_at(
                borrow(dir),
                PathFlags::SYMLINK_FOLLOW,
                path.into(),
                OpenFlags::empty(),
                flags,
            )
            .unwrap()
    }
    #[test]
    fn preopen_rejects_parent_and_absolute_escape_and_write_escalation() {
        let tmp = Scratch::new();
        std::fs::write(tmp.0.join("data"), b"kept").unwrap();
        let mut state = tmp.state(true);
        let root = root(&mut state);
        for path in ["../", "/etc/passwd"] {
            assert!(state
                .open_at(
                    borrow(&root),
                    PathFlags::SYMLINK_FOLLOW,
                    path.into(),
                    OpenFlags::empty(),
                    DescriptorFlags::READ
                )
                .is_err());
        }
        assert!(matches!(
            state.open_at(
                borrow(&root),
                PathFlags::empty(),
                "data".into(),
                OpenFlags::empty(),
                DescriptorFlags::WRITE
            ),
            Err(FsError::Code(ErrorCode::NotPermitted))
        ));
        assert!(matches!(
            state.create_directory_at(borrow(&root), "forbidden".into()),
            Err(FsError::Code(ErrorCode::NotPermitted))
        ));
        let file = open(&mut state, &root, "data", DescriptorFlags::READ);
        assert_eq!(
            HostDescriptor::read(&mut state, borrow(&file), 100, 0)
                .unwrap()
                .0,
            b"kept"
        );
        assert!(state.set_size(borrow(&file), 0).is_err());
        assert!(state
            .set_times(borrow(&file), NewTimestamp::Now, NewTimestamp::Now)
            .is_err());
        assert_eq!(std::fs::read(tmp.0.join("data")).unwrap(), b"kept");
    }
    #[cfg(unix)]
    #[test]
    fn symlink_escape_and_nofollow_do_not_open_ambient_files() {
        let inside = Scratch::new();
        let outside = Scratch::new();
        std::fs::write(outside.0.join("secret"), b"secret").unwrap();
        std::os::unix::fs::symlink(&outside.0, inside.0.join("escape")).unwrap();
        std::os::unix::fs::symlink("data", inside.0.join("local")).unwrap();
        std::fs::write(inside.0.join("data"), b"local").unwrap();
        let mut state = inside.state(false);
        let root = root(&mut state);
        assert!(state
            .open_at(
                borrow(&root),
                PathFlags::SYMLINK_FOLLOW,
                "escape/secret".into(),
                OpenFlags::empty(),
                DescriptorFlags::READ
            )
            .is_err());
        assert!(state
            .open_at(
                borrow(&root),
                PathFlags::empty(),
                "local".into(),
                OpenFlags::empty(),
                DescriptorFlags::READ
            )
            .is_err());
        let file = open(&mut state, &root, "local", DescriptorFlags::READ);
        assert_eq!(
            HostDescriptor::read(&mut state, borrow(&file), 100, 0)
                .unwrap()
                .0,
            b"local"
        );
    }
    #[test]
    fn file_streams_keep_independent_offsets_and_survive_descriptor_drop() {
        let tmp = Scratch::new();
        std::fs::write(tmp.0.join("data"), b"abcdef").unwrap();
        let mut state = tmp.state(false);
        let root = root(&mut state);
        let file = open(
            &mut state,
            &root,
            "data",
            DescriptorFlags::READ | DescriptorFlags::WRITE,
        );
        let first = state.read_via_stream(borrow(&file), 1).unwrap();
        let second = state.read_via_stream(borrow(&file), 3).unwrap();
        let append = state.append_via_stream(borrow(&file)).unwrap();
        HostDescriptor::drop(&mut state, file).unwrap();
        assert_eq!(
            HostInputStream::read(&mut state, borrow(&first), 2).unwrap(),
            b"bc"
        );
        assert_eq!(
            HostInputStream::read(&mut state, borrow(&second), 2).unwrap(),
            b"de"
        );
        assert_eq!(
            HostInputStream::read(&mut state, borrow(&first), 2).unwrap(),
            b"de"
        );
        HostOutputStream::blocking_write_and_flush(&mut state, borrow(&append), b"gh".to_vec())
            .unwrap();
        assert_eq!(std::fs::read(tmp.0.join("data")).unwrap(), b"abcdefgh");
        HostInputStream::drop(&mut state, first).unwrap();
        HostInputStream::drop(&mut state, second).unwrap();
        HostOutputStream::drop(&mut state, append).unwrap();
    }
    #[test]
    fn mutations_and_metadata_use_the_supplied_directory_capability() {
        let tmp = Scratch::new();
        let mut state = tmp.state(false);
        let root = root(&mut state);
        state
            .create_directory_at(borrow(&root), "sub".into())
            .unwrap();
        let file = state
            .open_at(
                borrow(&root),
                PathFlags::empty(),
                "data".into(),
                OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
                DescriptorFlags::READ | DescriptorFlags::WRITE,
            )
            .unwrap();
        HostDescriptor::write(&mut state, borrow(&file), b"abcd".to_vec(), 0).unwrap();
        state.set_size(borrow(&file), 3).unwrap();
        let before = state.metadata_hash(borrow(&file)).unwrap();
        state
            .rename_at(
                borrow(&root),
                "data".into(),
                borrow(&root),
                "sub/moved".into(),
            )
            .unwrap();
        let after = state
            .metadata_hash_at(borrow(&root), PathFlags::empty(), "sub/moved".into())
            .unwrap();
        assert_eq!((before.lower, before.upper), (after.lower, after.upper));
        assert_eq!(state.stat(borrow(&file)).unwrap().size, 3);
        state
            .set_times(
                borrow(&file),
                NewTimestamp::NoChange,
                NewTimestamp::Timestamp(Datetime {
                    seconds: 999_999,
                    nanoseconds: 1_000_000_000,
                }),
            )
            .unwrap();
        assert_eq!(
            state
                .stat(borrow(&file))
                .unwrap()
                .data_modification_timestamp
                .unwrap()
                .seconds,
            1_000_000
        );
        state
            .link_at(
                borrow(&root),
                PathFlags::empty(),
                "sub/moved".into(),
                borrow(&root),
                "linked".into(),
            )
            .unwrap();
        let linked = open(&mut state, &root, "linked", DescriptorFlags::READ);
        assert!(state
            .is_same_object(borrow(&file), borrow(&linked))
            .unwrap());
        HostDescriptor::drop(&mut state, file).unwrap();
        HostDescriptor::drop(&mut state, linked).unwrap();
        state
            .unlink_file_at(borrow(&root), "sub/moved".into())
            .unwrap();
        state
            .remove_directory_at(borrow(&root), "sub".into())
            .unwrap();
        let entries = state.read_directory(borrow(&root)).unwrap();
        let entry = state
            .read_directory_entry(borrow(&entries))
            .unwrap()
            .unwrap();
        assert_eq!(entry.name, "linked");
        assert!(state
            .read_directory_entry(borrow(&entries))
            .unwrap()
            .is_none());
        HostDirectoryEntryStream::drop(&mut state, entries).unwrap();
    }
    #[test]
    fn pollable_child_prevents_premature_stream_drop() {
        let mut state = State::new(&WasiOptions::default()).unwrap();
        let input: Resource<Input> = state.table.push(Input::Empty).unwrap();
        let pollable = HostInputStream::subscribe(&mut state, borrow(&input)).unwrap();
        assert!(HostInputStream::drop(&mut state, Resource::new_own(input.rep())).is_err());
        super::super::bindings::wasi::io::poll::HostPollable::drop(&mut state, pollable).unwrap();
        HostInputStream::drop(&mut state, input).unwrap();
        let output: Resource<Output> = state.table.push(Output::Sink).unwrap();
        assert!(HostOutputStream::blocking_write_zeroes_and_flush(
            &mut state,
            borrow(&output),
            4097
        )
        .is_err());
        assert_eq!(
            HostOutputStream::check_write(&mut state, borrow(&output)).unwrap(),
            65536
        );
    }
    #[test]
    fn open_modes_and_read_bounds_match_the_previous_provider() {
        let tmp = Scratch::new();
        std::fs::write(tmp.0.join("data"), vec![7; 70_000]).unwrap();
        let mut state = tmp.state(false);
        let root = root(&mut state);
        let file = state
            .open_at(
                borrow(&root),
                PathFlags::empty(),
                "data".into(),
                OpenFlags::EXCLUSIVE,
                DescriptorFlags::empty(),
            )
            .unwrap();
        assert_eq!(
            state.get_flags(borrow(&file)).unwrap(),
            DescriptorFlags::READ
        );
        assert_eq!(
            HostDescriptor::read(&mut state, borrow(&file), u64::MAX, 0)
                .unwrap()
                .0
                .len(),
            65_536
        );
        // A stream can be created from a read-only native descriptor; the
        // underlying write reports the failure when the guest actually writes.
        let stream = state.write_via_stream(borrow(&file), 0).unwrap();
        assert!(HostOutputStream::write(&mut state, borrow(&stream), vec![1]).is_err());
        HostOutputStream::drop(&mut state, stream).unwrap();
        let created = state
            .open_at(
                borrow(&root),
                PathFlags::empty(),
                "created".into(),
                OpenFlags::CREATE,
                DescriptorFlags::empty(),
            )
            .unwrap();
        assert_eq!(
            state.get_flags(borrow(&created)).unwrap(),
            DescriptorFlags::READ | DescriptorFlags::WRITE
        );
        assert_eq!(
            HostDescriptor::write(&mut state, borrow(&created), b"created".to_vec(), 0).unwrap(),
            7
        );
        for flag in [
            DescriptorFlags::FILE_INTEGRITY_SYNC,
            DescriptorFlags::DATA_INTEGRITY_SYNC,
            DescriptorFlags::REQUESTED_WRITE_SYNC,
        ] {
            assert!(matches!(
                state.open_at(
                    borrow(&root),
                    PathFlags::empty(),
                    "data".into(),
                    OpenFlags::empty(),
                    flag
                ),
                Err(FsError::Code(ErrorCode::Unsupported))
            ));
        }
    }

    #[test]
    fn directory_operations_keep_grants_and_wrong_type_errors() {
        let tmp = Scratch::new();
        std::fs::create_dir(tmp.0.join("child")).unwrap();
        let mut state = tmp.state(false);
        let root = root(&mut state);
        assert!(matches!(
            state.advise(borrow(&root), 0, 0, Advice::Normal),
            Err(FsError::Code(ErrorCode::BadDescriptor))
        ));
        assert!(matches!(
            state.read_via_stream(borrow(&root), 0),
            Err(FsError::Code(ErrorCode::IsDirectory))
        ));
        assert!(matches!(
            state.write_via_stream(borrow(&root), 0),
            Err(FsError::Code(ErrorCode::BadDescriptor))
        ));
        let child = state
            .open_at(
                borrow(&root),
                PathFlags::empty(),
                "child".into(),
                OpenFlags::DIRECTORY,
                DescriptorFlags::READ,
            )
            .unwrap();
        // The host's read-write preopen grant is inherited; opening a directory
        // in read mode does not change that grant in the previous provider.
        state
            .create_directory_at(borrow(&child), "nested".into())
            .unwrap();
        state
            .set_times(borrow(&child), NewTimestamp::Now, NewTimestamp::Now)
            .unwrap();
        assert!(tmp.0.join("child/nested").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn syncing_a_capability_directory_reopens_it_for_io() {
        let tmp = Scratch::new();
        let mut state = tmp.state(true);
        let root = root(&mut state);
        state.sync_data(borrow(&root)).unwrap();
        state.sync(borrow(&root)).unwrap();
    }
}
