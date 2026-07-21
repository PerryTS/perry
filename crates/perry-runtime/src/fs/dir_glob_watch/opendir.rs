use super::*;
// The fs-module helpers (`decode_path_value`, `build_dirent_object`,
// `DirentKind`, `build_dir_object`, `alloc_dir_state`, `build_fs_error_value*`,
// `fs_encoding_option`, `encoded_string_ptr`, `options_*`, `validate`,
// `metadata_*`, `build_stats_object`, `extract_closure_ptr`,
// `js_string_from_bytes`, ...) live in the parent `fs` module; the trunk
// reached them via `use super::*` when it was a direct child of `fs`. As a
// grandchild we glob the `fs` module directly. `fs_encoding_option` /
// `encoded_string_ptr` are private to `fs/mod.rs` but reachable here as a
// descendant module, so name them explicitly in case the glob skips privates.

use std::fs;

/// `fs.opendirSync(path)` — codegen emits a direct call to the unmangled
/// `js_fs_opendir_sync` symbol (runtime_decls/strings.rs). Without `#[no_mangle]`
/// the symbol is Rust-mangled and the linker can't resolve it, so any program
/// using `opendirSync` failed with `Undefined symbols: _js_fs_opendir_sync`
/// (#4003-sibling found via #3964). The async/promises Dir paths reach the
/// shared `js_fs_opendir_value` helper directly, which is why only the sync
/// entry point was affected.
#[no_mangle]
pub extern "C" fn js_fs_opendir_sync(path_value: f64) -> f64 {
    match js_fs_opendir_value(path_value) {
        Ok(dir) => dir,
        Err(err) => crate::exception::js_throw(err),
    }
}

pub(crate) fn js_fs_opendir_value(path_value: f64) -> Result<f64, f64> {
    js_fs_opendir_value_inner(path_value, false)
}

pub(crate) fn js_fs_opendir_value_with_path(path_value: f64) -> Result<f64, f64> {
    js_fs_opendir_value_inner(path_value, true)
}

fn js_fs_opendir_value_inner(path_value: f64, include_path: bool) -> Result<f64, f64> {
    validate::validate_path("path", path_value);
    unsafe {
        let path = match decode_path_value(path_value) {
            Some(path) => path,
            None => validate::throw_invalid_path_arg("path", path_value),
        };
        let read_dir = match fs::read_dir(&path) {
            Ok(read_dir) => read_dir,
            Err(err) => {
                return Err(if include_path {
                    build_fs_error_value(&err, "opendir", &path)
                } else {
                    build_fs_error_value_no_path(&err, "opendir")
                });
            }
        };
        let mut entries = Vec::new();
        let mut items: Vec<(String, std::fs::FileType)> = Vec::new();
        for entry in read_dir.flatten() {
            if let (Some(name), Ok(ft)) = (entry.file_name().to_str(), entry.file_type()) {
                items.push((name.to_string(), ft));
            }
        }
        items.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, ft) in items {
            entries.push(build_dirent_object(
                &name,
                &path,
                DirentKind::from_file_type(&ft),
            ));
        }
        Ok(build_dir_object(alloc_dir_state(entries), &path))
    }
}
