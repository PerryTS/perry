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
    mode: i32,
    filter: f64,
    sync_filter: bool,
    sync_symlink_resolution: bool,
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
        mode: options_number_field(options_value, b"mode").unwrap_or(0.0) as i32,
        filter: options_field_value(options_value, b"filter")
            .map(|v| f64::from_bits(v.bits()))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
        sync_filter: false,
        sync_symlink_resolution: false,
    }
}

fn resolve_copied_symlink_target(src: &Path, target: PathBuf, opts: FsCopyOptions) -> PathBuf {
    if opts.verbatim_symlinks {
        return target;
    }
    let target_is_relative = target.is_relative();
    let resolved = if target.is_absolute() {
        target
    } else if let Some(parent) = src.parent() {
        parent.join(target)
    } else {
        target
    };
    if opts.sync_symlink_resolution {
        fs::canonicalize(&resolved).unwrap_or(resolved)
    } else if target_is_relative {
        lexical_normalize_path(resolved)
    } else {
        resolved
    }
}

unsafe fn build_cp_error_value(
    message: &str,
    code: &'static str,
    syscall: Option<&'static str>,
    path: Option<String>,
) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_message(msg);
    crate::node_submodules::register_error_code_pub(msg, code);
    if let Some(syscall) = syscall {
        crate::node_submodules::register_error_syscall(msg, syscall);
    }
    if let Some(path) = path {
        crate::node_submodules::register_error_path(msg, path);
    }
    crate::value::js_nanbox_pointer(err as i64)
}

unsafe fn build_cp_type_error_value(message: &str, code: &'static str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::node_submodules::register_error_code_pub(msg, code);
    crate::value::js_nanbox_pointer(err as i64)
}

unsafe fn build_cp_eexist_error(dst: &Path) -> f64 {
    let path = dst.to_string_lossy().into_owned();
    let message =
        format!("Target already exists: cp returned EEXIST ({path} already exists) {path}");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_message(msg);
    crate::node_submodules::register_error_code_pub(msg, "ERR_FS_CP_EEXIST");
    crate::node_submodules::register_error_syscall(msg, "cp");
    crate::node_submodules::register_error_path(msg, path);
    #[cfg(unix)]
    crate::node_submodules::set_error_user_prop(err as usize, "errno", libc::EEXIST as f64);
    crate::value::js_nanbox_pointer(err as i64)
}

unsafe fn build_invalid_filter_return_error() -> f64 {
    build_cp_type_error_value(
        "Expected boolean to be returned from the \"filter\" function but got an instance of Promise.",
        "ERR_INVALID_RETURN_VALUE",
    )
}

unsafe fn promise_ptr_from_value(value: f64) -> Option<*mut crate::promise::Promise> {
    if crate::promise::js_value_is_promise(value) == 0 {
        return None;
    }
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if js_value.is_pointer() {
        Some(js_value.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise)
    } else {
        None
    }
}

unsafe fn copy_filter_promise_result(promise: *mut crate::promise::Promise) -> Result<f64, f64> {
    for _ in 0..64 {
        match crate::promise::js_promise_state(promise) {
            1 => return Ok(crate::promise::js_promise_value(promise)),
            2 => return Err(crate::promise::js_promise_reason(promise)),
            _ => {
                if crate::promise::js_promise_run_microtasks() == 0 {
                    break;
                }
            }
        }
    }
    match crate::promise::js_promise_state(promise) {
        1 => Ok(crate::promise::js_promise_value(promise)),
        2 => Err(crate::promise::js_promise_reason(promise)),
        _ => Ok(f64::from_bits(crate::value::TAG_TRUE)),
    }
}

pub(crate) fn copy_filter_allows(src: &Path, dst: &Path, opts: FsCopyOptions) -> Result<bool, f64> {
    let filter = extract_closure_ptr(opts.filter);
    if filter.is_null() {
        return Ok(true);
    }
    let src_string = src.to_string_lossy();
    let dst_string = dst.to_string_lossy();
    let s = js_string_from_bytes(src_string.as_bytes().as_ptr(), src_string.len() as u32);
    let src_value = crate::value::js_nanbox_string(s as i64);
    let s = js_string_from_bytes(dst_string.as_bytes().as_ptr(), dst_string.len() as u32);
    let dst_value = crate::value::js_nanbox_string(s as i64);
    let result = crate::closure::js_closure_call2(filter, src_value, dst_value);
    unsafe {
        if let Some(promise) = promise_ptr_from_value(result) {
            if opts.sync_filter {
                return Err(build_invalid_filter_return_error());
            }
            let value = copy_filter_promise_result(promise)?;
            return Ok(crate::value::js_is_truthy(value) != 0);
        }
    }
    Ok(crate::value::js_is_truthy(result) != 0)
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
    let _ = set_path_times_result(&dst_string, atime / 1000.0, mtime / 1000.0, !follow);
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
) -> Result<(), f64> {
    let _mode = opts.mode;
    if !copy_filter_allows(src, dst, opts)? {
        return Ok(());
    }
    match fs::symlink_metadata(dst) {
        Ok(meta) if meta.is_dir() => {
            return Err(unsafe {
                let message = format!(
                    "Cannot overwrite directory {} with non-directory {}",
                    dst.to_string_lossy(),
                    src.to_string_lossy()
                );
                build_cp_error_value(&message, "ERR_FS_CP_NON_DIR_TO_DIR", None, None)
            });
        }
        Ok(_) => {
            if !opts.force {
                if opts.error_on_exist {
                    return Err(unsafe { build_cp_eexist_error(dst) });
                }
                return Ok(());
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|err| unsafe {
                    build_fs_error_value(&err, "mkdir", &parent.to_string_lossy())
                })?;
            }
        }
        Err(err) => {
            return Err(unsafe { build_fs_error_value(&err, "lstat", &dst.to_string_lossy()) });
        }
    }

    fs::copy(src, dst).map_err(|err| unsafe {
        build_fs_error_value_with_dest(
            &err,
            "copyfile",
            &src.to_string_lossy(),
            &dst.to_string_lossy(),
        )
    })?;
    if opts.preserve_timestamps {
        copy_preserve_timestamps(src, dst, opts.dereference);
    }
    Ok(())
}

pub(crate) fn copy_symlink_with_options(
    src: &Path,
    dst: &Path,
    opts: FsCopyOptions,
) -> Result<(), f64> {
    copy_symlink_with_options_depth(src, dst, opts, 0)
}

fn copy_symlink_with_options_depth(
    src: &Path,
    dst: &Path,
    opts: FsCopyOptions,
    depth: u32,
) -> Result<(), f64> {
    if !copy_filter_allows(src, dst, opts)? {
        return Ok(());
    }
    if opts.dereference && !opts.sync_symlink_resolution {
        let target_meta = fs::metadata(src)
            .map_err(|err| unsafe { build_fs_error_value(&err, "stat", &src.to_string_lossy()) })?;
        if target_meta.is_dir() {
            copy_dir_recursive_depth(src, dst, opts, depth + 1)
        } else {
            copy_file_with_options(src, dst, opts)
        }
    } else {
        match fs::symlink_metadata(dst) {
            Ok(meta) if meta.is_dir() => {
                return Err(unsafe {
                    let message = format!(
                        "Cannot overwrite directory {} with non-directory {}",
                        dst.to_string_lossy(),
                        src.to_string_lossy()
                    );
                    build_cp_error_value(&message, "ERR_FS_CP_NON_DIR_TO_DIR", None, None)
                });
            }
            Ok(_) => {
                if !opts.force {
                    if opts.error_on_exist {
                        return Err(unsafe { build_cp_eexist_error(dst) });
                    }
                    return Ok(());
                }
                fs::remove_file(dst).map_err(|err| unsafe {
                    build_fs_error_value(&err, "unlink", &dst.to_string_lossy())
                })?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = dst.parent() {
                    fs::create_dir_all(parent).map_err(|err| unsafe {
                        build_fs_error_value(&err, "mkdir", &parent.to_string_lossy())
                    })?;
                }
            }
            Err(err) => {
                return Err(unsafe { build_fs_error_value(&err, "lstat", &dst.to_string_lossy()) });
            }
        }
        let target = fs::read_link(src).map_err(|err| unsafe {
            build_fs_error_value(&err, "readlink", &src.to_string_lossy())
        })?;
        let target = resolve_copied_symlink_target(src, target, opts);
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, dst).map_err(|err| unsafe {
            build_fs_error_value_with_dest(
                &err,
                "symlink",
                &src.to_string_lossy(),
                &dst.to_string_lossy(),
            )
        })?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, dst).map_err(|err| unsafe {
            build_fs_error_value_with_dest(
                &err,
                "symlink",
                &src.to_string_lossy(),
                &dst.to_string_lossy(),
            )
        })?;
        if opts.preserve_timestamps {
            copy_preserve_timestamps(src, dst, false);
        }
        Ok(())
    }
}

pub(crate) fn copy_dir_recursive(from: &Path, to: &Path, opts: FsCopyOptions) -> Result<(), f64> {
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
) -> Result<(), f64> {
    if depth >= COPY_DIR_MAX_DEPTH {
        return Err(unsafe {
            build_cp_error_value(
                "Directory nesting exceeds limit while copying (possible symlink cycle)",
                "ELOOP",
                Some("cp"),
                Some(from.to_string_lossy().into_owned()),
            )
        });
    }
    if !copy_filter_allows(from, to, opts)? {
        return Ok(());
    }
    match fs::symlink_metadata(to) {
        Ok(meta) if !meta.is_dir() => {
            return Err(unsafe {
                let message = format!(
                    "Cannot overwrite non-directory {} with directory {}",
                    to.to_string_lossy(),
                    from.to_string_lossy()
                );
                build_cp_error_value(&message, "ERR_FS_CP_DIR_TO_NON_DIR", None, None)
            });
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(to).map_err(|err| unsafe {
                build_fs_error_value(&err, "mkdir", &to.to_string_lossy())
            })?;
        }
        Err(err) => {
            return Err(unsafe { build_fs_error_value(&err, "lstat", &to.to_string_lossy()) });
        }
    }
    let entries = fs::read_dir(from)
        .map_err(|err| unsafe { build_fs_error_value(&err, "scandir", &from.to_string_lossy()) })?;
    for entry in entries {
        let entry = entry.map_err(|err| unsafe {
            build_fs_error_value(&err, "scandir", &from.to_string_lossy())
        })?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let file_type = entry.file_type().map_err(|err| unsafe {
            build_fs_error_value(&err, "lstat", &src.to_string_lossy())
        })?;
        if file_type.is_dir() {
            copy_dir_recursive_depth(&src, &dst, opts, depth + 1)?;
        } else if file_type.is_file() {
            copy_file_with_options(&src, &dst, opts)?;
        } else if file_type.is_symlink() {
            copy_symlink_with_options_depth(&src, &dst, opts, depth + 1)?;
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
    match js_fs_cp_options_result(from_value, to_value, options_value, true, true) {
        Ok(()) => 1,
        Err(err) => crate::exception::js_throw(err),
    }
}

pub(crate) fn js_fs_cp_async_options(from_value: f64, to_value: f64, options_value: f64) -> i32 {
    match js_fs_cp_async_result(from_value, to_value, options_value) {
        Ok(()) => 1,
        Err(err) => crate::exception::js_throw(err),
    }
}

pub(crate) fn js_fs_cp_async_result(
    from_value: f64,
    to_value: f64,
    options_value: f64,
) -> Result<(), f64> {
    js_fs_cp_options_result(from_value, to_value, options_value, false, false)
}

fn absolute_normalized_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    lexical_normalize_path(absolute)
}

fn path_is_inside(parent: &Path, child: &Path) -> bool {
    let parent_lex = absolute_normalized_path(parent);
    let child_abs = absolute_normalized_path(child);
    if child_abs != parent_lex && child_abs.starts_with(&parent_lex) {
        return true;
    }
    let parent_abs = fs::canonicalize(parent).unwrap_or_else(|_| absolute_normalized_path(parent));
    child_abs != parent_abs && child_abs.starts_with(&parent_abs)
}

unsafe fn build_cp_same_path_error(src: &Path) -> f64 {
    let message = format!("src and dest cannot be the same {}", src.to_string_lossy());
    build_cp_error_value(&message, "ERR_FS_CP_EINVAL", None, None)
}

unsafe fn build_cp_subdir_error(src: &Path, dst: &Path) -> f64 {
    let message = format!(
        "Cannot copy {} to a subdirectory of self {}",
        src.to_string_lossy(),
        dst.to_string_lossy()
    );
    build_cp_error_value(&message, "ERR_FS_CP_EINVAL", None, None)
}

unsafe fn build_cp_dir_to_non_dir_error(src: &Path, dst: &Path) -> f64 {
    let message = format!(
        "Cannot overwrite non-directory {} with directory {}",
        dst.to_string_lossy(),
        src.to_string_lossy()
    );
    build_cp_error_value(&message, "ERR_FS_CP_DIR_TO_NON_DIR", None, None)
}

unsafe fn build_cp_non_dir_to_dir_error(src: &Path, dst: &Path) -> f64 {
    let message = format!(
        "Cannot overwrite directory {} with non-directory {}",
        dst.to_string_lossy(),
        src.to_string_lossy()
    );
    build_cp_error_value(&message, "ERR_FS_CP_NON_DIR_TO_DIR", None, None)
}

unsafe fn build_cp_eisdir_error(src: &Path) -> f64 {
    let message = format!(
        "Recursive option not enabled, cannot copy a directory: {}/",
        src.to_string_lossy()
    );
    build_cp_error_value(&message, "ERR_FS_EISDIR", None, None)
}

fn js_fs_cp_options_result(
    from_value: f64,
    to_value: f64,
    options_value: f64,
    sync_symlink_resolution: bool,
    sync_filter: bool,
) -> Result<(), f64> {
    validate::validate_path("src", from_value);
    validate::validate_path("dest", to_value);
    validate::validate_object_options("options", options_value);
    unsafe {
        let from = match decode_path_value(from_value) {
            Some(s) => s,
            None => validate::throw_invalid_path_arg("src", from_value),
        };
        let to = match decode_path_value(to_value) {
            Some(s) => s,
            None => validate::throw_invalid_path_arg("dest", to_value),
        };
        let src = Path::new(&from);
        let dst = Path::new(&to);
        let mut opts = fs_copy_options_from_value(options_value);
        opts.sync_symlink_resolution = sync_symlink_resolution;
        opts.sync_filter = sync_filter;

        let src_meta = if opts.dereference {
            fs::metadata(src).map_err(|err| build_fs_error_value(&err, "stat", &from))?
        } else {
            fs::symlink_metadata(src).map_err(|err| build_fs_error_value(&err, "lstat", &from))?
        };
        let src_is_dir = src_meta.is_dir();
        if src_is_dir && !opts.recursive {
            return Err(build_cp_eisdir_error(src));
        }

        let dst_meta = match fs::symlink_metadata(dst) {
            Ok(meta) => Some(meta),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(build_fs_error_value(&err, "lstat", &to)),
        };
        if dst_meta.is_some() {
            if let (Ok(canon_src), Ok(canon_dst)) = (fs::canonicalize(src), fs::canonicalize(dst)) {
                if canon_src == canon_dst {
                    return Err(build_cp_same_path_error(src));
                }
            }
        }
        if src_is_dir && path_is_inside(src, dst) {
            return Err(build_cp_subdir_error(src, dst));
        }
        if let Some(dst_meta) = dst_meta {
            if src_is_dir && !dst_meta.is_dir() {
                return Err(build_cp_dir_to_non_dir_error(src, dst));
            }
            if !src_is_dir && dst_meta.is_dir() {
                return Err(build_cp_non_dir_to_dir_error(src, dst));
            }
        }

        if src_is_dir {
            copy_dir_recursive(src, dst, opts)
        } else if src_meta.file_type().is_symlink() {
            copy_symlink_with_options(src, dst, opts)
        } else {
            copy_file_with_options(src, dst, opts)
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
