//! cpSync / copy_dir_recursive + FsCopyOptions.

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::*;

#[derive(Clone, Copy)]
pub(crate) struct FsCopyOptions {
    force: bool,
    error_on_exist: bool,
    preserve_timestamps: bool,
    dereference: bool,
    verbatim_symlinks: bool,
    recursive: bool,
    filter: f64,
}

pub(crate) unsafe fn fs_copy_options_from_value(options_value: f64) -> FsCopyOptions {
    let force = if options_has_field(options_value, b"force") {
        options_bool_field(options_value, b"force")
    } else {
        true
    };
    FsCopyOptions {
        force,
        error_on_exist: options_bool_field(options_value, b"errorOnExist"),
        preserve_timestamps: options_bool_field(options_value, b"preserveTimestamps"),
        dereference: options_bool_field(options_value, b"dereference"),
        verbatim_symlinks: options_bool_field(options_value, b"verbatimSymlinks"),
        recursive: options_bool_field(options_value, b"recursive"),
        filter: options_field_value(options_value, b"filter")
            .map(|v| f64::from_bits(v.bits()))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
    }
}

pub(crate) fn copy_filter_allows(src: &Path, dst: &Path, opts: FsCopyOptions) -> bool {
    let filter = extract_closure_ptr(opts.filter);
    if filter.is_null() {
        return true;
    }
    let src_string = src.to_string_lossy();
    let dst_string = dst.to_string_lossy();
    let src_value = unsafe {
        let s = js_string_from_bytes(src_string.as_bytes().as_ptr(), src_string.len() as u32);
        crate::value::js_nanbox_string(s as i64)
    };
    let dst_value = unsafe {
        let s = js_string_from_bytes(dst_string.as_bytes().as_ptr(), dst_string.len() as u32);
        crate::value::js_nanbox_string(s as i64)
    };
    let result = crate::closure::js_closure_call2(filter, src_value, dst_value);
    crate::value::js_is_truthy(result) != 0
}

pub(crate) fn copy_preserve_timestamps(src: &Path, dst: &Path, follow: bool) {
    let meta = if follow {
        fs::metadata(src)
    } else {
        fs::symlink_metadata(src)
    };
    let Ok(meta) = meta else {
        return;
    };
    let (atime, mtime, _, _) = metadata_times_ms(&meta);
    let dst_string = dst.to_string_lossy();
    // `set_path_times` is unix-only (utimensat); on other targets timestamp
    // preservation is a no-op, matching the cfg-gated callers in fs/mod.rs.
    #[cfg(unix)]
    let _ = set_path_times(&dst_string, atime / 1000.0, mtime / 1000.0, !follow);
    #[cfg(not(unix))]
    let _ = (&dst_string, atime, mtime, follow);
}

pub(crate) fn lexical_normalize_path(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub(crate) fn copy_file_with_options(
    src: &Path,
    dst: &Path,
    opts: FsCopyOptions,
) -> std::io::Result<()> {
    if !copy_filter_allows(src, dst, opts) {
        return Ok(());
    }
    if dst.exists() {
        if !opts.force {
            if opts.error_on_exist {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "destination exists",
                ));
            }
            return Ok(());
        }
    } else if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(src, dst)?;
    if opts.preserve_timestamps {
        copy_preserve_timestamps(src, dst, opts.dereference);
    }
    Ok(())
}

pub(crate) fn copy_symlink_with_options(
    src: &Path,
    dst: &Path,
    opts: FsCopyOptions,
) -> std::io::Result<()> {
    if !copy_filter_allows(src, dst, opts) {
        return Ok(());
    }
    if opts.dereference {
        let target_meta = fs::metadata(src)?;
        if target_meta.is_dir() {
            copy_dir_recursive(src, dst, opts)
        } else {
            copy_file_with_options(src, dst, opts)
        }
    } else {
        if dst.exists() {
            if !opts.force {
                if opts.error_on_exist {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "destination exists",
                    ));
                }
                return Ok(());
            }
            let _ = fs::remove_file(dst);
        } else if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut target = fs::read_link(src)?;
        if !opts.verbatim_symlinks && target.is_relative() {
            if let Some(parent) = src.parent() {
                target = lexical_normalize_path(parent.join(target));
            }
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, dst)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, dst)?;
        if opts.preserve_timestamps {
            copy_preserve_timestamps(src, dst, false);
        }
        Ok(())
    }
}

pub(crate) fn copy_dir_recursive(
    from: &Path,
    to: &Path,
    opts: FsCopyOptions,
) -> std::io::Result<()> {
    copy_dir_recursive_depth(from, to, opts, 0)
}

// Guard against symlink cycles under `dereference: true`. Node's cp gives up
// with ELOOP via the OS; we bound depth defensively so a malicious tree can't
// stack-overflow Perry's process.
pub(crate) const COPY_DIR_MAX_DEPTH: u32 = 256;

pub(crate) fn copy_dir_recursive_depth(
    from: &Path,
    to: &Path,
    opts: FsCopyOptions,
    depth: u32,
) -> std::io::Result<()> {
    if depth >= COPY_DIR_MAX_DEPTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cpSync: directory nesting exceeds limit (possible symlink cycle)",
        ));
    }
    if !copy_filter_allows(from, to, opts) {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive_depth(&src, &dst, opts, depth + 1)?;
        } else if file_type.is_file() {
            copy_file_with_options(&src, &dst, opts)?;
        } else if file_type.is_symlink() {
            copy_symlink_with_options(&src, &dst, opts)?;
        }
    }
    if opts.preserve_timestamps {
        copy_preserve_timestamps(from, to, opts.dereference);
    }
    Ok(())
}

/// `fs.cpSync(from, to, { recursive: true })` — deterministic subset:
/// copies files, regular directory trees, and the most common
/// force/errorOnExist/preserveTimestamps/dereference options.
#[no_mangle]
pub extern "C" fn js_fs_cp_sync(from_value: f64, to_value: f64) -> i32 {
    js_fs_cp_sync_options(
        from_value,
        to_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    )
}

#[no_mangle]
pub extern "C" fn js_fs_cp_sync_options(from_value: f64, to_value: f64, options_value: f64) -> i32 {
    unsafe {
        let from = match decode_path_value(from_value) {
            Some(s) => s,
            None => return 0,
        };
        let to = match decode_path_value(to_value) {
            Some(s) => s,
            None => return 0,
        };
        let src = Path::new(&from);
        let dst = Path::new(&to);
        let opts = fs_copy_options_from_value(options_value);
        // Node throws ERR_FS_CP_EINVAL if `src == dest`. We don't propagate
        // typed errors yet, so return 0 (failure) to keep `cpSync` from
        // silently no-op'ing into itself.
        if let (Ok(canon_src), Ok(canon_dst)) = (fs::canonicalize(src), fs::canonicalize(dst)) {
            if canon_src == canon_dst {
                return 0;
            }
        }
        let meta = if opts.dereference {
            fs::metadata(src)
        } else {
            fs::symlink_metadata(src)
        };
        // Node requires `{ recursive: true }` to copy directories; otherwise
        // it throws ERR_FS_EISDIR. Surface the same gate via `js_throw` so
        // `try/catch` around `cpSync` actually fires.
        if matches!(meta, Ok(ref m) if m.is_dir()) && !opts.recursive {
            let bytes = b"ERR_FS_EISDIR: cpSync: src is a directory (use { recursive: true })";
            let msg = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let err = crate::error::js_error_new_with_message(msg);
            let err_val = crate::value::js_nanbox_pointer(err as i64);
            crate::exception::js_throw(err_val);
        }
        let result = match meta {
            Ok(meta) if meta.is_dir() => copy_dir_recursive(src, dst, opts),
            Ok(meta) if meta.file_type().is_symlink() => copy_symlink_with_options(src, dst, opts),
            Ok(_) => copy_file_with_options(src, dst, opts),
            Err(err) => Err(err),
        };
        if result.is_ok() {
            1
        } else {
            0
        }
    }
}

/// `fs.accessSync(path)` — returns 1 if accessible, 0 otherwise.
/// Unlike Node's `accessSync` which throws on failure, this returns a
/// status code; the LLVM codegen wraps the result so `try/catch` works.
#[no_mangle]
pub extern "C" fn js_fs_access_sync(path_value: f64) -> i32 {
    js_fs_access_sync_mode(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}
