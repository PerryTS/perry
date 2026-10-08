//! A source boundary: GC layout owns no address-keyed records.
use std::path::Path;

fn assert_layout_has_no_maps(dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            assert_layout_has_no_maps(&path);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let source = std::fs::read_to_string(&path).unwrap();
            for line in source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
            {
                // Also refuse aliases and inferred constructors: an owner map
                // must not hide behind a different key type or spelling.
                for token in ["HashMap", "BTreeMap", "new_ptr_hash_map"] {
                    assert!(
                        !line.contains(token),
                        "layout map in {}: {line}",
                        path.display()
                    );
                }
            }
        }
    }
}

#[test]
fn no_address_keyed_map_exists_under_gc_layout() {
    let gc = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gc");
    assert_layout_has_no_maps(&gc.join("layout"));
    for entry in std::fs::read_dir(&gc).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("layout")
            && path.is_file()
        {
            let source = std::fs::read_to_string(&path).unwrap();
            let source = source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<String>();
            for token in ["HashMap", "BTreeMap", "new_ptr_hash_map"] {
                assert!(!source.contains(token), "layout map in {}", path.display());
            }
        }
    }
    assert!(!gc.join("layout_tables.rs").exists());
}
