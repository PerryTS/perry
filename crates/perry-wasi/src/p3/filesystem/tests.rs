use super::{types::*, *};
use crate::filesystem::{Descriptor, FsPerms, WasiFilesystem, WasiFilesystemCtxView};
use crate::p3::tests::{with_store, State};
use crate::pipe::MemoryOutputPipe;
use futures_channel::oneshot;
use futures_lite::future::block_on;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use wasmtime::{
    component::{
        Accessor, FutureConsumer, FutureReader, Resource, Source, StreamConsumer, StreamReader,
        StreamResult,
    },
    StoreContextMut,
};

type Host = WasiFilesystem;
type Outcome = Result<(), ErrorCode>;
fn borrow<T: 'static>(resource: &Resource<T>) -> Resource<T> {
    Resource::new_borrow(resource.rep())
}
fn fs(accessor: &Accessor<State>) -> Accessor<State, WasiFilesystem> {
    accessor.with_getter(|state| WasiFilesystemCtxView {
        ctx: &mut state.ctx.as_mut().unwrap().filesystem,
        table: &mut state.table,
    })
}
fn preopen(accessor: &Accessor<State>) -> Resource<Descriptor> {
    let mut dirs = fs(accessor)
        .with(|mut store| preopens::Host::get_directories(&mut store.get()))
        .unwrap();
    assert_eq!(dirs.len(), 1);
    let (dir, name) = dirs.pop().unwrap();
    assert_eq!(name, "/data");
    dir
}
struct Receive(Option<oneshot::Sender<Outcome>>);
impl<D> FutureConsumer<D> for Receive {
    type Item = Outcome;
    fn poll_consume(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        store: StoreContextMut<D>,
        mut source: Source<Outcome>,
        _: bool,
    ) -> Poll<wasmtime::Result<()>> {
        let mut value = None;
        source.read(store, &mut value)?;
        if let Some(value) = value {
            let _ = self.0.take().unwrap().send(value);
        }
        Poll::Ready(Ok(()))
    }
}
fn receive(
    accessor: &Accessor<State>,
    future: FutureReader<Outcome>,
) -> oneshot::Receiver<Outcome> {
    let (send, receive) = oneshot::channel();
    accessor
        .with(|store| future.pipe(store, Receive(Some(send))))
        .unwrap();
    receive
}
fn bytes(
    accessor: &Accessor<State>,
    data: StreamReader<u8>,
) -> (MemoryOutputPipe, oneshot::Receiver<Outcome>) {
    let output = MemoryOutputPipe::new(4 * 1024 * 1024);
    let (send, receive) = oneshot::channel();
    accessor
        .with(|store| {
            data.pipe(
                store,
                crate::p3::streams::Output::new(Box::new(output.clone()), send),
            )
        })
        .unwrap();
    (output, receive)
}
struct Entries(Arc<Mutex<Vec<DirectoryEntry>>>);
impl<D> StreamConsumer<D> for Entries {
    type Item = DirectoryEntry;
    fn poll_consume(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        store: StoreContextMut<D>,
        mut source: Source<DirectoryEntry>,
        _: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let mut value = None;
        source.read(store, &mut value)?;
        if let Some(value) = value {
            self.0.lock().unwrap().push(value);
        }
        Poll::Ready(Ok(StreamResult::Completed))
    }
}
fn entries(
    accessor: &Accessor<State>,
    data: StreamReader<DirectoryEntry>,
) -> Arc<Mutex<Vec<DirectoryEntry>>> {
    let result = Arc::new(Mutex::new(Vec::new()));
    accessor
        .with(|store| data.pipe(store, Entries(result.clone())))
        .unwrap();
    result
}
async fn open(
    accessor: &Accessor<State, WasiFilesystem>,
    dir: &Resource<Descriptor>,
    name: &str,
    flags: DescriptorFlags,
) -> Resource<Descriptor> {
    Host::open_at(
        accessor,
        borrow(dir),
        PathFlags::empty(),
        name.into(),
        OpenFlags::empty(),
        flags,
    )
    .await
    .unwrap()
}

#[test]
fn large_files_stream_with_offsets_append_and_survive_descriptor_drop() {
    with_store(|mut store| {
        let temp = tempfile::tempdir().unwrap();
        let expected: Vec<u8> = (0..3 * 1024 * 1024 + 137)
            .map(|i| (i * 17 + 31) as u8)
            .collect();
        std::fs::write(temp.path().join("input"), &expected).unwrap();
        std::fs::write(temp.path().join("output"), b"prefix:").unwrap();
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .preopened_dir(temp.path(), "/data", FsPerms::ReadWrite)
            .unwrap();
        block_on(store.run_concurrent(async |accessor| {
            let dir = preopen(accessor);
            let fs = fs(accessor);
            let source = open(&fs, &dir, "input", DescriptorFlags::READ).await;
            let target = open(
                &fs,
                &dir,
                "output",
                DescriptorFlags::READ | DescriptorFlags::WRITE,
            )
            .await;
            let (data, read) = fs
                .with(|s| Host::read_via_stream(s, borrow(&source), 9))
                .unwrap();
            let written = fs
                .with(|s| Host::write_via_stream(s, borrow(&target), data, 7))
                .unwrap();
            // Stream work owns native handles independently of resource-table lifetime.
            fs.with(|mut s| HostDescriptor::drop(&mut s.get(), source))
                .unwrap();
            receive(accessor, read).await.unwrap().unwrap();
            receive(accessor, written).await.unwrap().unwrap();
            let data = accessor
                .with(|s| StreamReader::new(s, vec![42u8, 0, 255]))
                .unwrap();
            let appended = fs
                .with(|s| Host::append_via_stream(s, borrow(&target), data))
                .unwrap();
            receive(accessor, appended).await.unwrap().unwrap();
            assert_eq!(
                Host::stat(&fs, borrow(&target)).await.unwrap().size,
                (7 + expected.len() - 9 + 3) as u64
            );
            let (data, read) = fs
                .with(|s| Host::read_via_stream(s, borrow(&target), 0))
                .unwrap();
            fs.with(|mut s| HostDescriptor::drop(&mut s.get(), target))
                .unwrap();
            let (output, drained) = bytes(accessor, data);
            receive(accessor, read).await.unwrap().unwrap();
            drained.await.unwrap_or(Ok(())).unwrap();
            let mut wanted = b"prefix:".to_vec();
            wanted.extend_from_slice(&expected[9..]);
            wanted.extend_from_slice(&[42, 0, 255]);
            assert_eq!(output.contents().as_ref(), wanted);
            fs.with(|mut s| HostDescriptor::drop(&mut s.get(), dir))
                .unwrap();
        }))
        .unwrap();
        store.assert_concurrent_state_empty();
        assert_eq!(store.data().ctx.as_ref().unwrap().outstanding_io(), 0);
    });
}

#[test]
fn metadata_paths_and_directory_batches_preserve_capabilities() {
    with_store(|mut store| {
        let temp = tempfile::tempdir().unwrap();
        for i in 0..137 {
            std::fs::write(temp.path().join(format!("entry-{i:03}")), b"x").unwrap();
        }
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .preopened_dir(temp.path(), "/data", FsPerms::ReadWrite)
            .unwrap();
        block_on(store.run_concurrent(async |accessor| {
            let dir = preopen(accessor);
            let fs = fs(accessor);
            let (data, done) = fs.with(|s| Host::read_directory(s, borrow(&dir))).unwrap();
            let found = entries(accessor, data);
            receive(accessor, done).await.unwrap().unwrap();
            {
                let mut found = found.lock().unwrap();
                assert_eq!(found.len(), 137);
                found.sort_by(|a, b| a.name.cmp(&b.name));
                for (i, entry) in found.iter().enumerate() {
                    assert_eq!(entry.name, format!("entry-{i:03}"));
                    assert!(matches!(entry.type_, DescriptorType::RegularFile));
                }
            }
            let file = open(
                &fs,
                &dir,
                "entry-000",
                DescriptorFlags::READ | DescriptorFlags::WRITE,
            )
            .await;
            Host::advise(&fs, borrow(&file), 0, 1, Advice::Sequential)
                .await
                .unwrap();
            assert!(matches!(
                Host::get_type(&fs, borrow(&file)).await.unwrap(),
                DescriptorType::RegularFile
            ));
            assert!(Host::get_flags(&fs, borrow(&file))
                .await
                .unwrap()
                .contains(DescriptorFlags::READ | DescriptorFlags::WRITE));
            Host::set_size(&fs, borrow(&file), 42).await.unwrap();
            Host::sync_data(&fs, borrow(&file)).await.unwrap();
            Host::sync(&fs, borrow(&file)).await.unwrap();
            assert_eq!(
                Host::stat_at(&fs, borrow(&dir), PathFlags::empty(), "entry-000".into())
                    .await
                    .unwrap()
                    .size,
                42
            );
            let hash = Host::metadata_hash(&fs, borrow(&file)).await.unwrap();
            let path_hash =
                Host::metadata_hash_at(&fs, borrow(&dir), PathFlags::empty(), "entry-000".into())
                    .await
                    .unwrap();
            assert_eq!((hash.lower, hash.upper), (path_hash.lower, path_hash.upper));
            let second = open(&fs, &dir, "entry-000", DescriptorFlags::READ).await;
            assert!(Host::is_same_object(&fs, borrow(&file), borrow(&second))
                .await
                .unwrap());
            fs.with(|mut s| {
                HostDescriptor::drop(&mut s.get(), file)?;
                HostDescriptor::drop(&mut s.get(), second)
            })
            .unwrap();
            Host::create_directory_at(&fs, borrow(&dir), "child".into())
                .await
                .unwrap();
            Host::rename_at(
                &fs,
                borrow(&dir),
                "entry-000".into(),
                borrow(&dir),
                "child/renamed".into(),
            )
            .await
            .unwrap();
            Host::link_at(
                &fs,
                borrow(&dir),
                PathFlags::empty(),
                "child/renamed".into(),
                borrow(&dir),
                "linked".into(),
            )
            .await
            .unwrap();
            Host::unlink_file_at(&fs, borrow(&dir), "linked".into())
                .await
                .unwrap();
            Host::unlink_file_at(&fs, borrow(&dir), "child/renamed".into())
                .await
                .unwrap();
            Host::remove_directory_at(&fs, borrow(&dir), "child".into())
                .await
                .unwrap();
            for path in ["../escape", "/absolute"] {
                assert!(matches!(
                    Host::open_at(
                        &fs,
                        borrow(&dir),
                        PathFlags::empty(),
                        path.into(),
                        OpenFlags::empty(),
                        DescriptorFlags::READ
                    )
                    .await
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                    ErrorCode::NotPermitted
                ));
            }
            fs.with(|mut s| HostDescriptor::drop(&mut s.get(), dir))
                .unwrap();
        }))
        .unwrap();
        store.assert_concurrent_state_empty();
        assert_eq!(store.data().ctx.as_ref().unwrap().outstanding_io(), 0);
    });
}

#[test]
fn readonly_and_wrong_descriptor_errors_complete_typed_futures() {
    with_store(|mut store| {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("keep"), b"original").unwrap();
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .preopened_dir(temp.path(), "/data", FsPerms::ReadOnly)
            .unwrap();
        block_on(store.run_concurrent(async |accessor| {
            let dir = preopen(accessor);
            let fs = fs(accessor);
            let file = open(&fs, &dir, "keep", DescriptorFlags::READ).await;
            let data = accessor
                .with(|s| StreamReader::new(s, vec![1u8, 2, 3]))
                .unwrap();
            let outcome = fs
                .with(|s| Host::write_via_stream(s, borrow(&file), data, 0))
                .unwrap();
            assert!(matches!(
                receive(accessor, outcome).await.unwrap(),
                Err(ErrorCode::NotPermitted)
            ));
            assert!(matches!(
                Host::set_size(&fs, borrow(&file), 0)
                    .await
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::NotPermitted
            ));
            assert!(matches!(
                Host::create_directory_at(&fs, borrow(&dir), "denied".into())
                    .await
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::NotPermitted
            ));
            let (data, outcome) = fs
                .with(|s| Host::read_via_stream(s, borrow(&dir), 0))
                .unwrap();
            let (_, drained) = bytes(accessor, data);
            assert!(matches!(
                receive(accessor, outcome).await.unwrap(),
                Err(ErrorCode::IsDirectory)
            ));
            let _ = drained.await;
            let (data, outcome) = fs.with(|s| Host::read_directory(s, borrow(&file))).unwrap();
            let found = entries(accessor, data);
            assert!(matches!(
                receive(accessor, outcome).await.unwrap(),
                Err(ErrorCode::NotDirectory)
            ));
            assert!(found.lock().unwrap().is_empty());
            let (mut data, outcome) = fs
                .with(|s| Host::read_via_stream(s, borrow(&file), 0))
                .unwrap();
            accessor.with(|s| data.close(s)).unwrap();
            receive(accessor, outcome).await.unwrap().unwrap();
            fs.with(|mut s| {
                HostDescriptor::drop(&mut s.get(), file)?;
                HostDescriptor::drop(&mut s.get(), dir)
            })
            .unwrap();
            assert!(Host::stat(&fs, Resource::new_borrow(u32::MAX))
                .await
                .unwrap_err()
                .downcast()
                .is_err());
        }))
        .unwrap();
        assert_eq!(
            std::fs::read(temp.path().join("keep")).unwrap(),
            b"original"
        );
        store.assert_concurrent_state_empty();
    });
}

#[test]
fn signed_file_timestamps_and_stopped_driver_are_not_success_stubs() {
    with_store(|mut store| {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("file"), b"x").unwrap();
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .preopened_dir(temp.path(), "/data", FsPerms::ReadWrite)
            .unwrap();
        block_on(store.run_concurrent(async |accessor| {
            let dir = preopen(accessor);
            let fs = fs(accessor);
            let file = open(
                &fs,
                &dir,
                "file",
                DescriptorFlags::READ | DescriptorFlags::WRITE,
            )
            .await;
            let stamp = || {
                NewTimestamp::Timestamp(crate::p3::bindings::clocks::system_clock::Instant {
                    seconds: -3,
                    nanoseconds: 999_999_900,
                })
            };
            Host::set_times(&fs, borrow(&file), stamp(), stamp())
                .await
                .unwrap();
            let stat = Host::stat(&fs, borrow(&file)).await.unwrap();
            let modified = stat.data_modification_timestamp.unwrap();
            assert_eq!((modified.seconds, modified.nanoseconds), (-3, 999_999_900));
            Host::set_times_at(
                &fs,
                borrow(&dir),
                PathFlags::empty(),
                "file".into(),
                NewTimestamp::NoChange,
                NewTimestamp::Timestamp(crate::p3::bindings::clocks::system_clock::Instant {
                    seconds: 2,
                    nanoseconds: 100,
                }),
            )
            .await
            .unwrap();
            let modified = Host::stat(&fs, borrow(&file))
                .await
                .unwrap()
                .data_modification_timestamp
                .unwrap();
            assert_eq!((modified.seconds, modified.nanoseconds), (2, 100));
            // Retain the resource table but replace its driver's owner. The old
            // descriptors must fail rather than silently use the new driver.
            accessor.with(|mut s| s.get().ctx = Some(crate::WasiCtx::new(1).unwrap()));
            assert!(Host::stat(&fs, borrow(&file))
                .await
                .unwrap_err()
                .downcast()
                .is_err());
            fs.with(|mut s| {
                HostDescriptor::drop(&mut s.get(), file)?;
                HostDescriptor::drop(&mut s.get(), dir)
            })
            .unwrap();
        }))
        .unwrap();
        store.assert_concurrent_state_empty();
    });
}

#[cfg(unix)]
#[test]
fn symlink_escape_invalid_names_and_special_file_types() {
    use std::os::unix::{ffi::OsStringExt, net::UnixListener};
    with_store(|mut store| {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("file"), b"x").unwrap();
        let _socket = UnixListener::bind(temp.path().join("socket")).unwrap();
        // rustix::mkfifoat is unavailable on Apple targets. This fixture uses
        // an absolute path, so the portable POSIX mkfifo operation suffices.
        let fifo =
            std::ffi::CString::new(temp.path().join("fifo").into_os_string().into_vec()).unwrap();
        assert_eq!(
            unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) },
            0,
            "create FIFO fixture: {}",
            std::io::Error::last_os_error()
        );
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .preopened_dir(temp.path(), "/data", FsPerms::ReadWrite)
            .unwrap();
        block_on(store.run_concurrent(async |accessor| {
            let dir = preopen(accessor);
            let fs = fs(accessor);
            Host::symlink_at(&fs, borrow(&dir), "file".into(), "relative".into())
                .await
                .unwrap();
            assert_eq!(
                Host::readlink_at(&fs, borrow(&dir), "relative".into())
                    .await
                    .unwrap(),
                "file"
            );
            assert!(matches!(
                Host::stat_at(&fs, borrow(&dir), PathFlags::empty(), "relative".into())
                    .await
                    .unwrap()
                    .type_,
                DescriptorType::SymbolicLink
            ));
            assert!(matches!(
                Host::symlink_at(&fs, borrow(&dir), "/etc/passwd".into(), "escape".into())
                    .await
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::NotPermitted
            ));
            // An externally created symlink must also be rejected when followed.
            std::os::unix::fs::symlink("/etc/passwd", temp.path().join("escape")).unwrap();
            assert!(matches!(
                Host::open_at(
                    &fs,
                    borrow(&dir),
                    PathFlags::SYMLINK_FOLLOW,
                    "escape".into(),
                    OpenFlags::empty(),
                    DescriptorFlags::READ
                )
                .await
                .unwrap_err()
                .downcast()
                .unwrap(),
                ErrorCode::NotPermitted
            ));
            assert!(matches!(
                Host::stat_at(&fs, borrow(&dir), PathFlags::empty(), "fifo".into())
                    .await
                    .unwrap()
                    .type_,
                DescriptorType::Fifo
            ));
            assert!(matches!(
                Host::stat_at(&fs, borrow(&dir), PathFlags::empty(), "socket".into())
                    .await
                    .unwrap()
                    .type_,
                DescriptorType::Socket
            ));
            let invalid = std::ffi::OsString::from_vec(vec![b'b', 0xff]);
            std::fs::write(temp.path().join(invalid), b"x").unwrap();
            let (data, result) = fs.with(|s| Host::read_directory(s, borrow(&dir))).unwrap();
            let _entries = entries(accessor, data);
            assert!(matches!(
                receive(accessor, result).await.unwrap(),
                Err(ErrorCode::IllegalByteSequence)
            ));
            fs.with(|mut s| HostDescriptor::drop(&mut s.get(), dir))
                .unwrap();
        }))
        .unwrap();
        store.assert_concurrent_state_empty();
    });
}

#[test]
fn canonical_guest_transfers_filesystem_streams_and_error_futures() {
    use wasmtime::component::{Component, Linker};
    with_store(|mut store| {
        let temp = tempfile::tempdir().unwrap();
        let expected: Vec<u8> = (0..3 * 1024 * 1024 + 137)
            .map(|i| (i * 17 + 31) as u8)
            .collect();
        std::fs::write(temp.path().join("input"), &expected).unwrap();
        std::fs::write(temp.path().join("output"), []).unwrap();
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .preopened_dir(temp.path(), "/data", FsPerms::ReadWrite)
            .unwrap();
        let component =
            Component::new(store.engine(), include_str!("streams.component.wat")).unwrap();
        let mut linker = Linker::<State>::new(store.engine());
        crate::p3::add_to_linker(&mut linker).unwrap();
        block_on(async {
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let run = instance.get_typed_func::<(Resource<Descriptor>,Resource<Descriptor>),((FutureReader<Outcome>,FutureReader<Outcome>),)>(&mut store,"run").unwrap();
            store
                .run_concurrent(async |accessor| {
                    let dir = preopen(accessor);
                    let fs = fs(accessor);
                    let source = open(&fs, &dir, "input", DescriptorFlags::READ).await;
                    let target = open(&fs, &dir, "output", DescriptorFlags::WRITE).await;
                    let ((input, output),) = run
                        .call_concurrent(accessor, (borrow(&source), borrow(&target)))
                        .await
                        .unwrap();
                    receive(accessor, input).await.unwrap().unwrap();
                    receive(accessor, output).await.unwrap().unwrap();
                    let ((input, output),) = run
                        .call_concurrent(accessor, (borrow(&dir), borrow(&target)))
                        .await
                        .unwrap();
                    assert!(matches!(
                        receive(accessor, input).await.unwrap(),
                        Err(ErrorCode::IsDirectory)
                    ));
                    receive(accessor, output).await.unwrap().unwrap();
                    fs.with(|mut s| {
                        HostDescriptor::drop(&mut s.get(), source)?;
                        HostDescriptor::drop(&mut s.get(), target)?;
                        HostDescriptor::drop(&mut s.get(), dir)
                    })
                    .unwrap();
                })
                .await
                .unwrap();
        });
        assert_eq!(std::fs::read(temp.path().join("output")).unwrap(), expected);
        store.assert_concurrent_state_empty();
    });
}

#[test]
fn canonical_async_descriptor_calls_complete_and_reuse() {
    use wasmtime::component::{Component, Linker};
    with_store(|mut store| {
        let temp = tempfile::tempdir().unwrap();
        for name in ["one", "two"] {
            std::fs::write(temp.path().join(name), b"x").unwrap();
        }
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .preopened_dir(temp.path(), "/data", FsPerms::ReadWrite)
            .unwrap();
        let component =
            Component::new(store.engine(), include_str!("async.component.wat")).unwrap();
        let mut linker = Linker::<State>::new(store.engine());
        crate::p3::add_to_linker(&mut linker).unwrap();
        block_on(async {
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let run = instance
                .get_typed_func::<(Resource<Descriptor>, Resource<Descriptor>), (bool,)>(
                    &mut store, "run",
                )
                .unwrap();
            store
                .run_concurrent(async |accessor| {
                    let dir = preopen(accessor);
                    let fs = fs(accessor);
                    let one = open(&fs, &dir, "one", DescriptorFlags::READ).await;
                    let same = open(&fs, &dir, "one", DescriptorFlags::READ).await;
                    let other = open(&fs, &dir, "two", DescriptorFlags::READ).await;
                    for _ in 0..3 {
                        assert!(
                            run.call_concurrent(accessor, (borrow(&one), borrow(&same)))
                                .await
                                .unwrap()
                                .0
                        );
                        assert!(
                            !run.call_concurrent(accessor, (borrow(&one), borrow(&other)))
                                .await
                                .unwrap()
                                .0
                        );
                    }
                    fs.with(|mut s| {
                        HostDescriptor::drop(&mut s.get(), one)?;
                        HostDescriptor::drop(&mut s.get(), same)?;
                        HostDescriptor::drop(&mut s.get(), other)?;
                        HostDescriptor::drop(&mut s.get(), dir)
                    })
                    .unwrap();
                })
                .await
                .unwrap();
        });
        store.assert_concurrent_state_empty();
        assert_eq!(store.data().ctx.as_ref().unwrap().outstanding_io(), 0);
    });
}

#[test]
fn empty_files_chunk_boundaries_eof_and_closed_directory_streams() {
    with_store(|mut store| {
        let temp = tempfile::tempdir().unwrap();
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .preopened_dir(temp.path(), "/data", FsPerms::ReadWrite)
            .unwrap();
        block_on(store.run_concurrent(async |accessor| {
            let dir = preopen(accessor);
            let fs = fs(accessor);
            for length in [0, 65_536, 65_537] {
                let expected: Vec<u8> = (0..length).map(|i| (i * 13) as u8).collect();
                std::fs::write(temp.path().join("file"), &expected).unwrap();
                let file = open(&fs, &dir, "file", DescriptorFlags::READ).await;
                for offset in [0, length as u64, length as u64 + 1] {
                    let (data, result) = fs
                        .with(|s| Host::read_via_stream(s, borrow(&file), offset))
                        .unwrap();
                    let (output, done) = bytes(accessor, data);
                    receive(accessor, result).await.unwrap().unwrap();
                    done.await.unwrap_or(Ok(())).unwrap();
                    assert_eq!(
                        output.contents().as_ref(),
                        &expected[(offset as usize).min(length)..]
                    );
                }
                fs.with(|mut s| HostDescriptor::drop(&mut s.get(), file))
                    .unwrap();
            }
            let (mut data, result) = fs.with(|s| Host::read_directory(s, borrow(&dir))).unwrap();
            accessor.with(|s| data.close(s)).unwrap();
            receive(accessor, result).await.unwrap().unwrap();
            fs.with(|mut s| HostDescriptor::drop(&mut s.get(), dir))
                .unwrap();
        }))
        .unwrap();
        store.assert_concurrent_state_empty();
        assert_eq!(store.data().ctx.as_ref().unwrap().outstanding_io(), 0);
    });
}
