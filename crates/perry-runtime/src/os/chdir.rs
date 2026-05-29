//! `process.chdir()` and its Node-shaped error construction (#2135).
//!
//! Split out of `os.rs` to keep that file under the 2000-line gate. The
//! thrown error mirrors libuv's `chdir` shape:
//! `ENOENT: no such file or directory, chdir '<cwd>' -> '<target>'` with
//! `code` / `syscall` / `path` (= cwd, the *source*) / `dest` (= target) /
//! `errno` (negative libuv code) populated.

use crate::string::{js_string_from_bytes, StringHeader};

/// process.chdir(directory) — change working directory. Throws a Node-shaped
/// `Error` when the target can't be entered.
#[no_mangle]
pub extern "C" fn js_process_chdir(dir_ptr: *const StringHeader) {
    unsafe {
        if dir_ptr.is_null() {
            return;
        }
        let len = (*dir_ptr).byte_len as usize;
        let data = (dir_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        let Ok(target) = std::str::from_utf8(bytes) else {
            return;
        };
        if let Err(err) = std::env::set_current_dir(target) {
            throw_chdir_error(&err, target);
        }
    }
}

/// libuv-style POSIX `ERR_*` for an `io::Error`, scoped to the kinds
/// `chdir(2)` can return.
fn chdir_error_code(err: &std::io::Error) -> &'static str {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => "ENOENT",
        ErrorKind::PermissionDenied => "EACCES",
        ErrorKind::NotADirectory => "ENOTDIR",
        _ => "EIO",
    }
}

/// Human-readable description matching Node's libuv `chdir` error strings.
/// Node hardcodes these in its libuv error table; `io::Error::to_string()`
/// uses Rust's text (e.g. "entity not found"), which would diverge from
/// Node's output byte-for-byte.
fn chdir_error_description(code: &str) -> &'static str {
    match code {
        "ENOENT" => "no such file or directory",
        "EACCES" => "permission denied",
        "ENOTDIR" => "not a directory",
        _ => "i/o error",
    }
}

/// libuv `errno` (negative) for a `chdir` error code, matching the value Node
/// exposes on `err.errno`. libuv reports `-<system errno>`.
fn chdir_error_errno(code: &str) -> i32 {
    match code {
        "ENOENT" => -2,
        "EACCES" => -13,
        "ENOTDIR" => -20,
        _ => -5, // EIO
    }
}

unsafe fn throw_chdir_error(err: &std::io::Error, target: &str) -> ! {
    let code = chdir_error_code(err);
    let desc = chdir_error_description(code);
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let message = format!("{code}: {desc}, chdir '{cwd}' -> '{target}'");
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, code);
    crate::node_submodules::register_error_syscall(msg_ptr, "chdir");
    // #2135: match Node's uv `chdir` error shape — `path` is the *source*
    // (cwd), `dest` is the failed target, and `errno` is the negative libuv
    // code. Previously `path` was set to the target and `dest`/`errno` were
    // absent.
    crate::node_submodules::register_error_path(msg_ptr, cwd);
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    let err_addr = err_ptr as usize;
    let dest_ptr = js_string_from_bytes(target.as_ptr(), target.len() as u32);
    crate::node_submodules::set_error_user_prop(
        err_addr,
        "dest",
        crate::value::js_nanbox_string(dest_ptr as i64),
    );
    crate::node_submodules::set_error_user_prop(err_addr, "errno", chdir_error_errno(code) as f64);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chdir_error_errno_matches_libuv() {
        // #2135: Node exposes libuv's negative errno on chdir errors.
        assert_eq!(chdir_error_errno("ENOENT"), -2);
        assert_eq!(chdir_error_errno("EACCES"), -13);
        assert_eq!(chdir_error_errno("ENOTDIR"), -20);
        assert_eq!(chdir_error_errno("EIO"), -5);
    }
}
