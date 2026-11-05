// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use super::{preopens, time::systemtimespec_from, types, FilesystemError, FilesystemResult};
use crate::filesystem::{Descriptor, Dir, File, WasiFilesystem, WasiFilesystemCtxView};
use crate::p2::filesystem::{FileInputStream, FileOutputStream};
use crate::p3::streams;
use futures_channel::oneshot;
use types::{
    Advice, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry, ErrorCode, Filesize,
    MetadataHashValue, NewTimestamp, OpenFlags, PathFlags,
};
use wasmtime::component::{Access, Accessor, FutureReader, Resource, ResourceTable, StreamReader};
use wasmtime::error::Context as _;
fn get_descriptor<'a>(
    table: &'a ResourceTable,
    fd: &'a Resource<Descriptor>,
) -> FilesystemResult<&'a Descriptor> {
    table
        .get(fd)
        .context("failed to get descriptor resource from table")
        .map_err(FilesystemError::trap)
}

fn get_file<'a>(
    table: &'a ResourceTable,
    fd: &'a Resource<Descriptor>,
) -> FilesystemResult<&'a File> {
    let file = get_descriptor(table, fd)?.file()?;
    Ok(file)
}

fn get_dir<'a>(
    table: &'a ResourceTable,
    fd: &'a Resource<Descriptor>,
) -> FilesystemResult<&'a Dir> {
    let dir = get_descriptor(table, fd)?.dir()?;
    Ok(dir)
}

trait AccessorExt {
    fn get_descriptor(&self, fd: &Resource<Descriptor>) -> FilesystemResult<Descriptor>;
    fn get_file(&self, fd: &Resource<Descriptor>) -> FilesystemResult<File>;
    fn get_dir(&self, fd: &Resource<Descriptor>) -> FilesystemResult<Dir>;
    fn get_dir_pair(
        &self,
        a: &Resource<Descriptor>,
        b: &Resource<Descriptor>,
    ) -> FilesystemResult<(Dir, Dir)>;
}

impl<T> AccessorExt for Accessor<T, WasiFilesystem> {
    fn get_descriptor(&self, fd: &Resource<Descriptor>) -> FilesystemResult<Descriptor> {
        self.with(|mut store| {
            let fd = get_descriptor(store.get().table, fd)?;
            Ok(fd.clone())
        })
    }

    fn get_file(&self, fd: &Resource<Descriptor>) -> FilesystemResult<File> {
        self.with(|mut store| {
            let file = get_file(store.get().table, fd)?;
            Ok(file.clone())
        })
    }

    fn get_dir(&self, fd: &Resource<Descriptor>) -> FilesystemResult<Dir> {
        self.with(|mut store| {
            let dir = get_dir(store.get().table, fd)?;
            Ok(dir.clone())
        })
    }

    fn get_dir_pair(
        &self,
        a: &Resource<Descriptor>,
        b: &Resource<Descriptor>,
    ) -> FilesystemResult<(Dir, Dir)> {
        self.with(|mut store| {
            let table = store.get().table;
            let a = get_dir(table, a)?;
            let b = get_dir(table, b)?;
            Ok((a.clone(), b.clone()))
        })
    }
}

impl types::Host for WasiFilesystemCtxView<'_> {
    fn convert_error_code(&mut self, error: FilesystemError) -> wasmtime::Result<ErrorCode> {
        error.downcast()
    }
}
impl<U> types::HostDescriptorWithStore<U> for WasiFilesystem {
    fn read_via_stream(
        mut store: Access<U, Self>,
        fd: Resource<Descriptor>,
        offset: Filesize,
    ) -> wasmtime::Result<(StreamReader<u8>, FutureReader<Result<(), ErrorCode>>)> {
        let descriptor = get_descriptor(store.get().table, &fd)?.clone();
        let (send, receive) = oneshot::channel();
        let data = match descriptor {
            Descriptor::File(file) => StreamReader::new(
                &mut store,
                streams::Input::new(Box::new(FileInputStream::new(&file, offset)), send),
            )?,
            Descriptor::Dir(_) => {
                let _ = send.send(Err(ErrorCode::IsDirectory));
                StreamReader::new(&mut store, std::iter::empty())?
            }
        };
        Ok((
            data,
            FutureReader::new(&mut store, async move {
                wasmtime::error::Ok(receive.await.unwrap_or(Ok(())))
            })?,
        ))
    }
    fn write_via_stream(
        store: Access<U, Self>,
        fd: Resource<Descriptor>,
        data: StreamReader<u8>,
        offset: Filesize,
    ) -> wasmtime::Result<FutureReader<Result<(), ErrorCode>>> {
        write(store, fd, data, Some(offset))
    }
    fn append_via_stream(
        store: Access<U, Self>,
        fd: Resource<Descriptor>,
        data: StreamReader<u8>,
    ) -> wasmtime::Result<FutureReader<Result<(), ErrorCode>>> {
        write(store, fd, data, None)
    }
    fn read_directory(
        mut store: Access<U, Self>,
        fd: Resource<Descriptor>,
    ) -> wasmtime::Result<(
        StreamReader<DirectoryEntry>,
        FutureReader<Result<(), ErrorCode>>,
    )> {
        let descriptor = get_descriptor(store.get().table, &fd)?.clone();
        let (send, receive) = oneshot::channel();
        let data = match descriptor.dir() {
            Ok(dir) => StreamReader::new(
                &mut store,
                super::directory::Directory::new(dir.clone(), send),
            )?,
            Err(error) => {
                let _ = send.send(Err(FilesystemError::from(error).downcast()?));
                StreamReader::new(&mut store, std::iter::empty())?
            }
        };
        Ok((
            data,
            FutureReader::new(&mut store, async move {
                wasmtime::error::Ok(receive.await.unwrap_or(Ok(())))
            })?,
        ))
    }
    async fn advise(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        offset: Filesize,
        length: Filesize,
        advice: Advice,
    ) -> FilesystemResult<()> {
        let file = store.get_file(&fd)?;
        file.advise(offset, length, advice.into()).await?;
        Ok(())
    }

    async fn sync_data(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> FilesystemResult<()> {
        let fd = store.get_descriptor(&fd)?;
        fd.sync_data().await?;
        Ok(())
    }

    async fn get_flags(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> FilesystemResult<DescriptorFlags> {
        let fd = store.get_descriptor(&fd)?;
        let flags = fd.get_flags().await?;
        Ok(flags.into())
    }

    async fn get_type(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> FilesystemResult<DescriptorType> {
        let fd = store.get_descriptor(&fd)?;
        let ty = fd.get_type().await?;
        Ok(ty.into())
    }

    async fn set_size(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        size: Filesize,
    ) -> FilesystemResult<()> {
        let file = store.get_file(&fd)?;
        file.set_size(size).await?;
        Ok(())
    }

    async fn set_times(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        data_access_timestamp: NewTimestamp,
        data_modification_timestamp: NewTimestamp,
    ) -> FilesystemResult<()> {
        let fd = store.get_descriptor(&fd)?;
        let atim = systemtimespec_from(data_access_timestamp)?;
        let mtim = systemtimespec_from(data_modification_timestamp)?;
        fd.set_times(atim, mtim).await?;
        Ok(())
    }

    async fn sync(store: &Accessor<U, Self>, fd: Resource<Descriptor>) -> FilesystemResult<()> {
        let fd = store.get_descriptor(&fd)?;
        fd.sync().await?;
        Ok(())
    }

    async fn create_directory_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path: String,
    ) -> FilesystemResult<()> {
        let dir = store.get_dir(&fd)?;
        dir.create_directory_at(path).await?;
        Ok(())
    }

    async fn stat(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> FilesystemResult<DescriptorStat> {
        let fd = store.get_descriptor(&fd)?;
        let stat = fd.stat().await?;
        Ok(stat.into())
    }

    async fn stat_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
    ) -> FilesystemResult<DescriptorStat> {
        let dir = store.get_dir(&fd)?;
        let stat = dir.stat_at(path_flags.into(), path).await?;
        Ok(stat.into())
    }

    async fn set_times_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
        data_access_timestamp: NewTimestamp,
        data_modification_timestamp: NewTimestamp,
    ) -> FilesystemResult<()> {
        let dir = store.get_dir(&fd)?;
        let atim = systemtimespec_from(data_access_timestamp)?;
        let mtim = systemtimespec_from(data_modification_timestamp)?;
        dir.set_times_at(path_flags.into(), path, atim, mtim)
            .await?;
        Ok(())
    }

    async fn link_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        old_path_flags: PathFlags,
        old_path: String,
        new_fd: Resource<Descriptor>,
        new_path: String,
    ) -> FilesystemResult<()> {
        let (old_dir, new_dir) = store.get_dir_pair(&fd, &new_fd)?;
        old_dir
            .link_at(old_path_flags.into(), old_path, &new_dir, new_path)
            .await?;
        Ok(())
    }

    async fn open_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
        open_flags: OpenFlags,
        flags: DescriptorFlags,
    ) -> FilesystemResult<Resource<Descriptor>> {
        let dir = store.get_dir(&fd)?;
        let fd = dir
            .open_at(path_flags.into(), path, open_flags.into(), flags.into())
            .await?;
        let fd = store.with(|mut store| store.get().table.push(fd))?;
        Ok(fd)
    }

    async fn readlink_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path: String,
    ) -> FilesystemResult<String> {
        let dir = store.get_dir(&fd)?;
        let path = dir.readlink_at(path).await?;
        Ok(path)
    }

    async fn remove_directory_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path: String,
    ) -> FilesystemResult<()> {
        let dir = store.get_dir(&fd)?;
        dir.remove_directory_at(path).await?;
        Ok(())
    }

    async fn rename_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        old_path: String,
        new_fd: Resource<Descriptor>,
        new_path: String,
    ) -> FilesystemResult<()> {
        let (old_dir, new_dir) = store.get_dir_pair(&fd, &new_fd)?;
        old_dir.rename_at(old_path, &new_dir, new_path).await?;
        Ok(())
    }

    async fn symlink_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        old_path: String,
        new_path: String,
    ) -> FilesystemResult<()> {
        let dir = store.get_dir(&fd)?;
        dir.symlink_at(old_path, new_path).await?;
        Ok(())
    }

    async fn unlink_file_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path: String,
    ) -> FilesystemResult<()> {
        let dir = store.get_dir(&fd)?;
        dir.unlink_file_at(path).await?;
        Ok(())
    }

    async fn is_same_object(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        other: Resource<Descriptor>,
    ) -> wasmtime::Result<bool> {
        let (fd, other) = store.with(|mut store| {
            let table = store.get().table;
            let fd = get_descriptor(table, &fd)?.clone();
            let other = get_descriptor(table, &other)?.clone();
            wasmtime::error::Ok((fd, other))
        })?;
        fd.is_same_object(&other).await
    }

    async fn metadata_hash(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> FilesystemResult<MetadataHashValue> {
        let fd = store.get_descriptor(&fd)?;
        let meta = fd.metadata_hash().await?;
        Ok(meta.into())
    }

    async fn metadata_hash_at(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
    ) -> FilesystemResult<MetadataHashValue> {
        let dir = store.get_dir(&fd)?;
        let meta = dir.metadata_hash_at(path_flags.into(), path).await?;
        Ok(meta.into())
    }
}

impl types::HostDescriptor for WasiFilesystemCtxView<'_> {
    fn drop(&mut self, fd: Resource<Descriptor>) -> wasmtime::Result<()> {
        self.table
            .delete(fd)
            .context("failed to delete descriptor resource from table")?;
        Ok(())
    }
}

impl preopens::Host for WasiFilesystemCtxView<'_> {
    fn get_directories(&mut self) -> wasmtime::Result<Vec<(Resource<Descriptor>, String)>> {
        self.get_directories()
    }
}

fn write<U>(
    mut store: Access<U, WasiFilesystem>,
    fd: Resource<Descriptor>,
    mut data: StreamReader<u8>,
    offset: Option<u64>,
) -> wasmtime::Result<FutureReader<Result<(), ErrorCode>>> {
    // Invalid resource IDs trap. Only filesystem capability failures are typed.
    let descriptor = get_descriptor(store.get().table, &fd)?.clone();
    let file = descriptor
        .file()
        .map_err(FilesystemError::from)
        .and_then(|file| {
            if file.perms.write_not_permitted() {
                Err(ErrorCode::NotPermitted.into())
            } else {
                Ok(file)
            }
        });
    let (send, receive) = oneshot::channel();
    match file {
        Ok(file) => {
            let output = match offset {
                Some(offset) => FileOutputStream::write_at(file, offset),
                None => FileOutputStream::append(file),
            };
            data.pipe(&mut store, streams::Output::new(Box::new(output), send))?;
        }
        Err(error) => {
            data.close(&mut store)?;
            let _ = send.send(Err(error.downcast()?));
        }
    }
    FutureReader::new(&mut store, async move {
        wasmtime::error::Ok(receive.await.unwrap_or(Ok(())))
    })
}
