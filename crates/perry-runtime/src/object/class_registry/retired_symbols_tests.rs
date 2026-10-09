//! #10502 S7: the class-table retirement deleted the process-wide latches and
//! generation counters that let class-id shortcuts pretend a class chain was
//! pristine. Every resolution now compares the ShapeIds of the objects it
//! read (receiver, hop and holder shapes), so none of them may come back:
//! this test fails if any of the deleted names reappears in the runtime or
//! codegen sources (test files aside, which may name them to assert their
//! absence).
//!
//! The names are spelled in pieces so this file does not match itself.

use std::path::{Path, PathBuf};

fn retired_names() -> Vec<String> {
    [
        ["VTABLE", "_GEN"].concat(),
        ["CLASS_LOOKUP", "_SURFACE_GEN"].concat(),
        ["class_lookup_surface", "_generation"].concat(),
        ["class_lookup_surface", "_gen_bump"].concat(),
        ["lookup_class_method", "_in_chain"].concat(),
        ["method_owner", "_class_id"].concat(),
        ["class_instance_has", "_member"].concat(),
        ["instance_chain_parent", "_class_id"].concat(),
        ["_method_guard", "_slot"].concat(),
        ["class_registry", "_inert"].concat(),
        ["class_decl_prototype", "_relinked"].concat(),
        ["class_private_accessor", "_decl"].concat(),
        ["class_decl_prototype_method", "_names"].concat(),
        ["class_chain", "_declares"].concat(),
        ["class_method_entry", "_value"].concat(),
        ["vtable", "_generation"].concat(),
        ["FAST_GUARDS", "_INVALIDATED"].concat(),
        ["invalidate_class_prototype", "_fast_guards"].concat(),
        ["class_prototype_method", "_guard_slot"].concat(),
        ["invalidate_prototype", "_descriptor_guards"].concat(),
        ["prototype_relink_may", "_retarget_direct_arms"].concat(),
        ["retire_prototype_caches", "_without_direct_arms"].concat(),
        ["CLASS_CHAIN", "_RELINKED_EVER"].concat(),
        ["any_class_chain", "_relinked"].concat(),
        ["note_class_chain", "_relinked"].concat(),
        ["relinked_instance", "_chain_answer"].concat(),
        ["relinked_object", "_chain_answer"].concat(),
    ]
    .into_iter()
    .collect()
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "tests") {
                continue;
            }
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs")
            && !path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("_tests.rs") || n == "tests.rs")
        {
            out.push(path);
        }
    }
}

#[test]
fn retired_class_table_latches_stay_deleted() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [runtime.join("src"), runtime.join("../perry-codegen/src")];
    let mut files = Vec::new();
    for root in &roots {
        rust_sources(root, &mut files);
    }
    assert!(
        files.len() > 500,
        "the scan must see the runtime and codegen sources, saw {} files",
        files.len()
    );
    let names = retired_names();
    let mut hits = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        for (line_no, line) in text.lines().enumerate() {
            for name in &names {
                if line.contains(name.as_str()) {
                    hits.push(format!("{}:{}: {}", file.display(), line_no + 1, name));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "retired class-table names reintroduced (#10502 S7):\n{}",
        hits.join("\n")
    );
}
