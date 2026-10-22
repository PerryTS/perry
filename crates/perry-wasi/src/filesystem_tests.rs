use crate::filesystem::{FsPerms, WasiFilesystemCtxView};
use crate::p2::bindings::filesystem::types::{HostDescriptor, HostDirectoryEntryStream};
use crate::p2::bindings::filesystem::{preopens, types};
use crate::{p2::FsError, WasiCtx};
use futures_lite::future::block_on;
use wasmtime::component::{Resource, ResourceTable};
use wasmtime_wasi_io::streams::{InputStream, OutputStream, StreamError};

fn borrow<T: 'static>(r: &Resource<T>) -> Resource<T> {
    Resource::new_borrow(r.rep())
}
fn code(err: FsError, expected: types::ErrorCode) {
    assert_eq!(err.downcast().unwrap(), expected);
}

#[test]
fn filesystem_host_roundtrip_offsets_metadata_and_directory_entries() {
    block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let mut ctx = WasiCtx::new(8).unwrap();
        ctx.preopened_dir(temp.path(), "/data", FsPerms::ReadWrite)
            .unwrap();
        let mut table = ResourceTable::new();
        let mut view = WasiFilesystemCtxView {
            ctx: &mut ctx.filesystem,
            table: &mut table,
        };
        let mut preopens = preopens::Host::get_directories(&mut view).unwrap();
        assert_eq!(preopens.len(), 1);
        let (dir, name) = preopens.pop().unwrap();
        assert_eq!(name, "/data");
        let file = view
            .open_at(
                borrow(&dir),
                types::PathFlags::empty(),
                "payload".into(),
                types::OpenFlags::CREATE | types::OpenFlags::EXCLUSIVE,
                types::DescriptorFlags::READ | types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();
        assert_eq!(
            view.write(borrow(&file), b"Turnloop filesystem".to_vec(), 0)
                .await
                .unwrap(),
            19
        );
        assert_eq!(
            view.read(borrow(&file), 10, 9).await.unwrap(),
            (b"filesystem".to_vec(), false)
        );
        assert_eq!(
            view.read(borrow(&file), 10, 19).await.unwrap(),
            (vec![], true)
        );
        assert_eq!(view.stat(borrow(&file)).await.unwrap().size, 19);
        let stat = view
            .stat_at(borrow(&dir), types::PathFlags::empty(), "payload".into())
            .await
            .unwrap();
        assert_eq!(stat.type_, types::DescriptorType::RegularFile);
        let hash = view.metadata_hash(borrow(&file)).await.unwrap();
        let path_hash = view
            .metadata_hash_at(borrow(&dir), types::PathFlags::empty(), "payload".into())
            .await
            .unwrap();
        assert_eq!((hash.lower, hash.upper), (path_hash.lower, path_hash.upper));
        let entries = view.read_directory(borrow(&dir)).await.unwrap();
        let entry = view
            .read_directory_entry(borrow(&entries))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.name, "payload");
        assert_eq!(entry.type_, types::DescriptorType::RegularFile);
        assert!(view
            .read_directory_entry(borrow(&entries))
            .await
            .unwrap()
            .is_none());
        HostDirectoryEntryStream::drop(&mut view, entries).unwrap();
        view.set_size(borrow(&file), 8).await.unwrap();
        view.sync(borrow(&file)).await.unwrap();
        assert_eq!(
            std::fs::read(temp.path().join("payload")).unwrap(),
            b"Turnloop"
        );
        HostDescriptor::drop(&mut view, file).unwrap();
        view.rename_at(
            borrow(&dir),
            "payload".into(),
            borrow(&dir),
            "renamed".into(),
        )
        .await
        .unwrap();
        view.unlink_file_at(borrow(&dir), "renamed".into())
            .await
            .unwrap();
        view.create_directory_at(borrow(&dir), "child".into())
            .await
            .unwrap();
        view.remove_directory_at(borrow(&dir), "child".into())
            .await
            .unwrap();
        HostDescriptor::drop(&mut view, dir).unwrap();
        assert_eq!(ctx.outstanding_io(), 0);
    });
}

#[test]
fn readonly_preopen_denies_mutation_and_preserves_typed_errors() {
    block_on(async {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("keep"), b"original").unwrap();
        let mut ctx = WasiCtx::new(4).unwrap();
        ctx.preopened_dir(temp.path(), ".", FsPerms::ReadOnly)
            .unwrap();
        let mut table = ResourceTable::new();
        let mut view = WasiFilesystemCtxView {
            ctx: &mut ctx.filesystem,
            table: &mut table,
        };
        let (dir, _) = preopens::Host::get_directories(&mut view)
            .unwrap()
            .pop()
            .unwrap();
        let f = view
            .open_at(
                borrow(&dir),
                types::PathFlags::empty(),
                "keep".into(),
                types::OpenFlags::empty(),
                types::DescriptorFlags::READ,
            )
            .await
            .unwrap();
        assert_eq!(view.read(borrow(&f), 8, 0).await.unwrap().0, b"original");
        code(
            view.write(borrow(&f), b"damage".to_vec(), 0)
                .await
                .unwrap_err(),
            types::ErrorCode::NotPermitted,
        );
        code(
            view.set_size(borrow(&f), 0).await.unwrap_err(),
            types::ErrorCode::NotPermitted,
        );
        code(
            view.create_directory_at(borrow(&dir), "new".into())
                .await
                .unwrap_err(),
            types::ErrorCode::NotPermitted,
        );
        code(
            view.unlink_file_at(borrow(&dir), "keep".into())
                .await
                .unwrap_err(),
            types::ErrorCode::NotPermitted,
        );
        code(
            view.open_at(
                borrow(&dir),
                types::PathFlags::empty(),
                "keep".into(),
                types::OpenFlags::TRUNCATE,
                types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap_err(),
            types::ErrorCode::NotPermitted,
        );
        code(
            view.stat_at(borrow(&dir), types::PathFlags::empty(), "missing".into())
                .await
                .unwrap_err(),
            types::ErrorCode::NoEntry,
        );
        assert_eq!(
            std::fs::read(temp.path().join("keep")).unwrap(),
            b"original"
        );
    });
}

#[test]
fn capability_paths_cannot_escape_preopen() {
    block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let inside = temp.path().join("inside");
        std::fs::create_dir(&inside).unwrap();
        let outside = temp.path().join("secret");
        std::fs::write(&outside, b"outside capability").unwrap();
        let mut ctx = WasiCtx::new(4).unwrap();
        ctx.preopened_dir(&inside, ".", FsPerms::ReadWrite).unwrap();
        let mut table = ResourceTable::new();
        let mut view = WasiFilesystemCtxView {
            ctx: &mut ctx.filesystem,
            table: &mut table,
        };
        let (dir, _) = preopens::Host::get_directories(&mut view)
            .unwrap()
            .pop()
            .unwrap();
        for path in [
            "../secret".to_owned(),
            outside.to_string_lossy().into_owned(),
        ] {
            let err = view
                .open_at(
                    borrow(&dir),
                    types::PathFlags::SYMLINK_FOLLOW,
                    path,
                    types::OpenFlags::empty(),
                    types::DescriptorFlags::READ,
                )
                .await
                .unwrap_err();
            assert!(matches!(
                err.downcast().unwrap(),
                types::ErrorCode::NotPermitted | types::ErrorCode::Access
            ));
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("../secret", inside.join("escape")).unwrap();
            code(
                view.open_at(
                    borrow(&dir),
                    types::PathFlags::SYMLINK_FOLLOW,
                    "escape".into(),
                    types::OpenFlags::empty(),
                    types::DescriptorFlags::READ,
                )
                .await
                .unwrap_err(),
                types::ErrorCode::NotPermitted,
            );
        }
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside capability");
    });
}

#[test]
fn file_streams_use_native_workers_and_keep_offsets() {
    block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let native = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(temp.path().join("stream"))
            .unwrap();
        let driver = perry_component_io::Driver::new(4).unwrap();
        let file = crate::filesystem::File::new(
            native,
            FsPerms::ReadWrite,
            crate::filesystem::OpenMode::READ | crate::filesystem::OpenMode::WRITE,
            driver.submitter(),
        );
        let mut output = crate::p2::filesystem::FileOutputStream::write_at(&file, 0);
        assert!(output.check_write().unwrap() >= 5);
        output.write(bytes::Bytes::from_static(b"first")).unwrap();
        assert_eq!(output.check_write().unwrap(), 0);
        wasmtime_wasi_io::poll::Pollable::ready(&mut output).await;
        output
            .blocking_write_and_flush(bytes::Bytes::from_static(b"second"))
            .await
            .unwrap();
        let mut input = crate::p2::filesystem::FileInputStream::new(&file, 3);
        assert!(input.blocking_read(0).await.unwrap().is_empty());
        assert_eq!(input.blocking_read(5).await.unwrap().as_ref(), b"stsec");
        assert_eq!(input.blocking_read(10).await.unwrap().as_ref(), b"ond");
        assert!(matches!(
            input.blocking_read(1).await,
            Err(StreamError::Closed)
        ));
        let mut append = crate::p2::filesystem::FileOutputStream::append(&file);
        append
            .blocking_write_and_flush(bytes::Bytes::from_static(b"!"))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(temp.path().join("stream")).unwrap(),
            b"firstsecond!"
        );
        input.cancel().await;
        output.cancel().await;
        append.cancel().await;
        assert_eq!(driver.outstanding(), 0);
    });
}

#[test]
fn driver_failure_is_a_trap_for_descriptors_and_streams() {
    block_on(async {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("file"), b"bytes").unwrap();
        let mut ctx = WasiCtx::new(4).unwrap();
        ctx.preopened_dir(temp.path(), ".", FsPerms::ReadOnly)
            .unwrap();
        let mut table = ResourceTable::new();
        let mut view = WasiFilesystemCtxView {
            ctx: &mut ctx.filesystem,
            table: &mut table,
        };
        let (dir, _) = preopens::Host::get_directories(&mut view)
            .unwrap()
            .pop()
            .unwrap();
        let f = view
            .open_at(
                borrow(&dir),
                types::PathFlags::empty(),
                "file".into(),
                types::OpenFlags::empty(),
                types::DescriptorFlags::READ,
            )
            .await
            .unwrap();
        let native_file = view.table.get(&f).unwrap().file().unwrap().clone();
        let mut input = crate::p2::filesystem::FileInputStream::new(&native_file, 0);
        let mut fs = std::mem::take(&mut ctx.filesystem);
        drop(ctx);
        let mut view = WasiFilesystemCtxView {
            ctx: &mut fs,
            table: &mut table,
        };
        let err = view.stat(borrow(&f)).await.unwrap_err();
        assert!(err
            .downcast()
            .unwrap_err()
            .downcast_ref::<perry_component_io::Error>()
            .is_some());
        wasmtime_wasi_io::poll::Pollable::ready(&mut input).await;
        assert!(matches!(input.read(1), Err(StreamError::Trap(_))));
        assert!(matches!(input.read(1), Err(StreamError::Trap(_))));
        let err = view.stat(Resource::new_borrow(u32::MAX)).await.unwrap_err();
        assert!(err.downcast().is_err());
    });
}

#[test]
fn components_receive_owned_preopens_through_sync_and_async_linkers() {
    struct State {
        ctx: WasiCtx,
        table: ResourceTable,
    }
    impl crate::WasiView for State {
        fn ctx(&mut self) -> crate::WasiCtxView<'_> {
            crate::WasiCtxView {
                ctx: &mut self.ctx,
                table: &mut self.table,
            }
        }
    }
    for asynchronous in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let engine = wasmtime::Engine::default();
        let component =
            wasmtime::component::Component::new(&engine, include_str!("filesystem.component.wat"))
                .unwrap();
        let mut linker = wasmtime::component::Linker::new(&engine);
        if asynchronous {
            crate::p2::add_to_linker_async(&mut linker).unwrap();
        } else {
            crate::p2::add_to_linker_sync(&mut linker).unwrap();
        }
        let mut ctx = WasiCtx::new(4).unwrap();
        ctx.preopened_dir(temp.path(), "/component-data", FsPerms::ReadOnly)
            .unwrap();
        let mut store = wasmtime::Store::new(
            &engine,
            State {
                ctx,
                table: ResourceTable::new(),
            },
        );
        let instance = if asynchronous {
            block_on(linker.instantiate_async(&mut store, &component)).unwrap()
        } else {
            linker.instantiate(&mut store, &component).unwrap()
        };
        let directories = instance
            .get_typed_func::<(), (Vec<(wasmtime::component::ResourceAny, String)>,)>(
                &mut store,
                "directories",
            )
            .unwrap();
        let (mut dirs,) = if asynchronous {
            block_on(directories.call_async(&mut store, ())).unwrap()
        } else {
            directories.call(&mut store, ()).unwrap()
        };
        assert_eq!(dirs.len(), 1);
        let (dir, name) = dirs.pop().unwrap();
        assert_eq!(name, "/component-data");
        assert!(dir.owned());
        if asynchronous {
            block_on(dir.resource_drop_async(&mut store)).unwrap();
        } else {
            dir.resource_drop(&mut store).unwrap();
        }
        assert_eq!(store.data().ctx.outstanding_io(), 0);
    }
}
