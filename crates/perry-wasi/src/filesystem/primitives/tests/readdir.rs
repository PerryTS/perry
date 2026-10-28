// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use crate::filesystem::primitives::{open_ambient_dir, read_base_dir, DirEntry};
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

#[test]
fn test_dir_entries() {
    let tmpdir = tempfile::tempdir().expect("construct tempdir");

    let entries = dir_entries(tmpdir.path());
    assert_eq!(entries.len(), 0, "empty dir");

    let _f1 = std::fs::File::create(tmpdir.path().join("file1")).expect("create file1");

    let entries = dir_entries(tmpdir.path());
    assert!(
        entries.contains_key("file1"),
        "directory contains `file1`: {entries:?}"
    );
    assert_eq!(entries.len(), 1);

    let _f2 = std::fs::File::create(tmpdir.path().join("file2")).expect("create file1");
    let entries = dir_entries(tmpdir.path());
    assert!(
        entries.contains_key("file1"),
        "directory contains `file1`: {entries:?}"
    );
    assert!(
        entries.contains_key("file2"),
        "directory contains `file2`: {entries:?}"
    );
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_reread_entries() {
    let tmpdir = tempfile::tempdir().expect("construct tempdir");
    let dir = open_ambient_dir(tmpdir.path()).unwrap();

    let entries = read_entries(&dir);
    assert_eq!(entries.len(), 0, "empty dir");

    let _f1 = std::fs::File::create(tmpdir.path().join("file1")).expect("create file1");

    let entries = read_entries(&dir);
    assert!(
        entries.contains_key("file1"),
        "directory contains `file1`: {entries:?}"
    );
    assert_eq!(entries.len(), 1);

    let _f2 = std::fs::File::create(tmpdir.path().join("file2")).expect("create file1");
    let entries = read_entries(&dir);
    assert!(
        entries.contains_key("file1"),
        "directory contains `file1`: {entries:?}"
    );
    assert!(
        entries.contains_key("file2"),
        "directory contains `file2`: {entries:?}"
    );
    assert_eq!(entries.len(), 2);
}

fn dir_entries(path: &Path) -> HashMap<String, DirEntry> {
    let dir = open_ambient_dir(path).unwrap();
    read_entries(&dir)
}

fn read_entries(dir: &File) -> HashMap<String, DirEntry> {
    let mut out = HashMap::new();
    for e in read_base_dir(dir).unwrap() {
        let e = e.expect("non-error entry");
        let name = e.file_name().to_str().expect("utf8 filename").to_owned();
        assert!(!out.contains_key(&name), "name already read: {name}");
        out.insert(name, e);
    }
    out
}
