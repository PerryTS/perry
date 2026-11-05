#![cfg(unix)]

use super::*;
use std::collections::BTreeMap;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::time::SystemTime;

fn owned_root(label: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    for _ in 0..64 {
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "perry-cp-{label}-{}-{id}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        match fs::create_dir(&root) {
            Ok(()) => {
                fs::write(root.join("OWNER"), "perry-runtime fs::cp::tests\n").unwrap();
                let root = fs::canonicalize(root).unwrap();
                eprintln!("owned cp unit scratch: {}", root.display());
                return root;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot create owned cp fixture: {error}"),
        }
    }
    panic!("cannot allocate a unique owned cp fixture");
}

fn text(value: &str) -> f64 {
    crate::value::js_nanbox_string(js_string_from_bytes(value.as_ptr(), value.len() as u32) as i64)
}

/// Drive the public sync entry and the result path used by fs.cp's callback
/// facade. Both decode the same real JS options object and perform real I/O.
fn copy(src: &Path, dst: &Path, flags: &[(&'static str, bool)], callback: bool) -> Result<(), f64> {
    copy_with_filter(src, dst, flags, callback, None)
}

fn copy_with_filter(
    src: &Path,
    dst: &Path,
    flags: &[(&'static str, bool)],
    callback: bool,
    filter: Option<*const crate::closure::ClosureHeader>,
) -> Result<(), f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let filter = filter.map(|filter| scope.root_raw_const_ptr(filter));
    let src = scope.root_nanbox_f64(text(&src.to_string_lossy()));
    let dst = scope.root_nanbox_f64(text(&dst.to_string_lossy()));
    let options = scope.root_raw_mut_ptr(crate::object::js_object_alloc_null_proto(0, 0));
    for &(name, enabled) in flags {
        let key = crate::string::intern_ascii_literal(name.as_bytes());
        options.with_mut_ptr(|object| {
            crate::object::js_object_set_field_by_name(
                object,
                key,
                f64::from_bits(crate::value::JSValue::bool(enabled).bits()),
            );
        });
    }
    if let Some(filter) = filter {
        let key = crate::string::intern_ascii_literal(b"filter");
        options.with_mut_ptr(|object| {
            crate::object::js_object_set_field_by_name(
                object,
                key,
                crate::value::js_nanbox_pointer(
                    filter.get_raw_const_ptr::<crate::closure::ClosureHeader>() as i64,
                ),
            );
        });
    }
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        options.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64,
    ));
    if callback {
        js_fs_cp_async_result(
            src.get_nanbox_f64(),
            dst.get_nanbox_f64(),
            options.get_nanbox_f64(),
        )
    } else {
        crate::exception::catch_js_throw(|| {
            if flags.is_empty() {
                js_fs_cp_sync(src.get_nanbox_f64(), dst.get_nanbox_f64())
            } else {
                js_fs_cp_sync_options(
                    src.get_nanbox_f64(),
                    dst.get_nanbox_f64(),
                    options.get_nanbox_f64(),
                )
            }
        })
        .map(|result| assert_eq!(result, 1))
    }
}

fn error_code(error: f64) -> String {
    let scope = crate::gc::RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(error);
    let key = scope.root_string_ptr(crate::string::intern_ascii_literal(b"code"));
    let code = key.with_const_ptr(|key| {
        crate::object::js_object_get_field_by_name_f64(
            crate::value::js_nanbox_get_pointer(error.get_nanbox_f64())
                as *const crate::object::ObjectHeader,
            key,
        )
    });
    crate::fs::validate::read_js_string_pub(code)
}

#[derive(Debug, PartialEq)]
struct Entry {
    mode: u32,
    modified: SystemTime,
    bytes: Option<Vec<u8>>,
    link: Option<PathBuf>,
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        entries.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            Entry {
                mode: metadata.permissions().mode(),
                modified: metadata.modified().unwrap(),
                bytes: if metadata.is_file() {
                    Some(fs::read(path).unwrap())
                } else {
                    None
                },
                link: if metadata.file_type().is_symlink() {
                    Some(fs::read_link(path).unwrap())
                } else {
                    None
                },
            },
        );
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn source_tree(root: &Path) -> PathBuf {
    let src = root.join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("plain"), b"plain-data").unwrap();
    fs::write(root.join("target-file"), b"target-data").unwrap();
    fs::set_permissions(root.join("target-file"), fs::Permissions::from_mode(0o640)).unwrap();
    fs::create_dir(root.join("target-dir")).unwrap();
    fs::write(root.join("target-dir/child"), b"nested-data").unwrap();
    symlink("../target-file", src.join("file-link")).unwrap();
    symlink("../target-dir", src.join("dir-link")).unwrap();
    src
}

#[test]
fn cp_sync_recursive_dereference_materializes_file_and_directory_targets() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::gc_init();
    let root = owned_root("sync-dereference");
    let src = source_tree(&root);
    for target in [root.join("target-file"), root.join("target-dir")] {
        set_path_times_result(&target.to_string_lossy(), 981173106.0, 981173106.0, false).unwrap();
    }
    let before = snapshot(&src);
    let dest = root.join("dest");
    copy(
        &src,
        &dest,
        &[
            ("recursive", true),
            ("dereference", true),
            ("preserveTimestamps", true),
        ],
        false,
    )
    .unwrap();
    assert!(fs::symlink_metadata(dest.join("file-link"))
        .unwrap()
        .is_file());
    assert!(fs::symlink_metadata(dest.join("dir-link"))
        .unwrap()
        .is_dir());
    assert_eq!(fs::read(dest.join("file-link")).unwrap(), b"target-data");
    assert_eq!(
        fs::read(dest.join("dir-link/child")).unwrap(),
        b"nested-data"
    );
    assert_eq!(fs::read(dest.join("plain")).unwrap(), b"plain-data");
    for (copied, original) in [("file-link", "target-file"), ("dir-link", "target-dir")] {
        let copied = fs::metadata(dest.join(copied)).unwrap();
        let original = fs::metadata(root.join(original)).unwrap();
        assert_eq!(copied.modified().unwrap(), original.modified().unwrap());
        assert_eq!(
            copied.permissions().mode() & 0o777,
            original.permissions().mode() & 0o777
        );
    }
    assert_eq!(
        snapshot(&src),
        before,
        "source links/data/modes/mtime stay intact"
    );
    assert_eq!(fs::read(root.join("target-file")).unwrap(), b"target-data");
}

#[test]
fn cp_default_and_callback_result_preserve_existing_link_modes() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::gc_init();
    let root = owned_root("existing-modes");
    let src = source_tree(&root);
    copy(&src.join("plain"), &root.join("default-file"), &[], false).unwrap();
    assert_eq!(fs::read(root.join("default-file")).unwrap(), b"plain-data");
    for (name, flags, callback, preserved, verbatim) in [
        ("sync", vec![("recursive", true)], false, true, false),
        (
            "verbatim",
            vec![("recursive", true), ("verbatimSymlinks", true)],
            false,
            true,
            true,
        ),
        ("callback", vec![("recursive", true)], true, true, false),
        (
            "callback-dereference",
            vec![("recursive", true), ("dereference", true)],
            true,
            false,
            false,
        ),
    ] {
        let dest = root.join(name);
        copy(&src, &dest, &flags, callback).unwrap();
        assert_eq!(
            fs::symlink_metadata(dest.join("file-link"))
                .unwrap()
                .file_type()
                .is_symlink(),
            preserved,
            "{name}"
        );
        assert_eq!(
            fs::symlink_metadata(dest.join("dir-link"))
                .unwrap()
                .file_type()
                .is_symlink(),
            preserved,
            "{name}"
        );
        assert_eq!(fs::read(dest.join("file-link")).unwrap(), b"target-data");
        assert_eq!(
            fs::read(dest.join("dir-link/child")).unwrap(),
            b"nested-data"
        );
        if preserved {
            assert_eq!(
                fs::read_link(dest.join("file-link")).unwrap(),
                if verbatim {
                    PathBuf::from("../target-file")
                } else {
                    root.join("target-file")
                }
            );
        }
    }
}

#[test]
fn cp_rejected_existing_target_preserves_source_and_destination() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::gc_init();
    for callback in [false, true] {
        let root = owned_root("reject-existing");
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("dest")).unwrap();
        fs::write(root.join("target"), b"source-data").unwrap();
        symlink("../target", root.join("src/link")).unwrap();
        fs::write(root.join("dest/link"), b"keep-data").unwrap();
        let before = snapshot(&root);
        let flags = [("recursive", true), ("dereference", true), ("force", false)];
        copy(&root.join("src"), &root.join("dest"), &flags, callback).unwrap();
        assert_eq!(
            snapshot(&root),
            before,
            "force:false skips without mutation"
        );
        let flags = [
            ("recursive", true),
            ("dereference", true),
            ("force", false),
            ("errorOnExist", true),
        ];
        let error = copy(&root.join("src"), &root.join("dest"), &flags, callback).unwrap_err();
        assert_eq!(error_code(error), "ERR_FS_CP_EEXIST");
        assert_eq!(
            snapshot(&root),
            before,
            "rejected copy must not alter either side"
        );
        copy(
            &root.join("src"),
            &root.join("dest"),
            &[("recursive", true), ("dereference", true), ("force", true)],
            callback,
        )
        .unwrap();
        assert_eq!(fs::read(root.join("dest/link")).unwrap(), b"source-data");
        assert_eq!(fs::read(root.join("target")).unwrap(), b"source-data");
        assert!(fs::symlink_metadata(root.join("src/link"))
            .unwrap()
            .file_type()
            .is_symlink());
    }
}

#[test]
fn cp_rejected_dangling_dereference_preserves_both_trees() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::gc_init();
    for callback in [false, true] {
        let root = owned_root("reject-dangling");
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("dest")).unwrap();
        symlink("../missing", root.join("src/link")).unwrap();
        let before = snapshot(&root);
        let error = copy(
            &root.join("src"),
            &root.join("dest"),
            &[("recursive", true), ("dereference", true)],
            callback,
        )
        .unwrap_err();
        assert_eq!(error_code(error), "ENOENT");
        assert_eq!(
            snapshot(&root),
            before,
            "failed stat must precede every write"
        );
    }
}

thread_local! {
    static FILTER_CALLS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

extern "C" fn reject_link_filter(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    src: f64,
    _dst: f64,
) -> f64 {
    let name = crate::fs::validate::read_js_string_pub(src);
    let allowed = !name.ends_with("/skip-link");
    FILTER_CALLS.with(|calls| calls.borrow_mut().push(name));
    f64::from_bits(crate::value::JSValue::bool(allowed).bits())
}

#[test]
fn cp_dereference_filters_each_entry_once_before_following_links() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::gc_init();
    for callback in [false, true] {
        let root = owned_root("filter-once");
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("target"), b"source-data").unwrap();
        symlink("../target", root.join("src/link")).unwrap();
        symlink("../missing", root.join("src/skip-link")).unwrap();
        let scope = crate::gc::RuntimeHandleScope::new();
        let filter = scope.root_raw_const_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(reject_link_filter, 2),
            0,
        ));
        FILTER_CALLS.with(|calls| calls.borrow_mut().clear());
        copy_with_filter(
            &root.join("src"),
            &root.join("dest"),
            &[("recursive", true), ("dereference", true)],
            callback,
            Some(filter.get_raw_const_ptr()),
        )
        .unwrap();
        let mut calls = FILTER_CALLS.with(|calls| calls.borrow().clone());
        calls.sort();
        let mut expected = vec![
            root.join("src"),
            root.join("src/link"),
            root.join("src/skip-link"),
        ]
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(
            calls, expected,
            "one invocation per entry, including rejected dangling link"
        );
        assert!(fs::symlink_metadata(root.join("dest/link"))
            .unwrap()
            .is_file());
        assert_eq!(fs::read(root.join("dest/link")).unwrap(), b"source-data");
        assert!(!root.join("dest/skip-link").exists());
        assert!(fs::symlink_metadata(root.join("src/skip-link"))
            .unwrap()
            .file_type()
            .is_symlink());
    }
}
