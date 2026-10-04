//! The worker entry table: which worker files were compiled into this binary.
//!
//! The compiler gives every worker module it can name an init function
//! (`<module>__init`). The entry module's `main` registers each one here under
//! its absolute paths before any module runs. A Worker whose filename the
//! compiler could not match at the call site (`new ns.Worker(url)` on a
//! `getBuiltinModule("node:worker_threads")` namespace, a URL passed through
//! helpers, a path built at run time) is started by looking its path up in this
//! table. A path with no entry throws `ERR_WORKER_NOT_COMPILED` right away.
//!
//! The table only holds code addresses and is filled before user code runs, so
//! every thread (including a worker that starts nested workers) can read it.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, RwLock};

use perry_runtime::value::JSValue;

use super::{get_object_field_from_value, is_undefined, string_value_to_string};

extern "C" {
    fn js_url_file_url_to_path(url: f64, options: f64) -> f64;
}

static WORKER_ENTRIES: LazyLock<RwLock<HashMap<PathBuf, usize>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register one compiled worker entry under an absolute path. Called from the
/// generated `main` before any module init.
///
/// # Safety
/// `path_ptr` must point to `path_len` valid UTF-8 bytes.
#[no_mangle]
pub unsafe extern "C" fn js_worker_threads_register_entry(
    path_ptr: *const u8,
    path_len: i64,
    init: i64,
) {
    if path_ptr.is_null() || path_len <= 0 || init == 0 {
        return;
    }
    let bytes = std::slice::from_raw_parts(path_ptr, path_len as usize);
    let Ok(path) = std::str::from_utf8(bytes) else {
        return;
    };
    WORKER_ENTRIES
        .write()
        .unwrap()
        .insert(normalize(Path::new(path)), init as usize);
}

/// `new Worker(filename, options)` when the compiler could not pick the entry
/// at the call site. Normalizes `filename` as Node does, finds the compiled
/// entry and starts it, or throws.
#[no_mangle]
pub extern "C" fn js_worker_threads_worker_new_by_spec(filename: f64, options: f64) -> f64 {
    if !is_undefined(options) {
        let eval = get_object_field_from_value(options, "eval");
        if perry_runtime::value::js_is_truthy(eval) != 0 {
            throw_with_code(
                "worker_threads: an eval Worker needs source code that is known when the \
                 program is compiled; this one is built at run time"
                    .to_string(),
                "ERR_WORKER_EVAL_NOT_COMPILED",
                0,
            );
        }
    }
    let path = worker_path(filename);
    let init = lookup(&path).unwrap_or_else(|| {
        throw_with_code(
            format!(
                "worker_threads: no worker entry for {} was compiled into this binary. \
                 Name the worker file with new URL(\"<literal>\", import.meta.url) or a \
                 string literal so it is compiled in",
                path.display()
            ),
            "ERR_WORKER_NOT_COMPILED",
            0,
        )
    });
    super::js_worker_threads_worker_new(init as i64, options)
}

/// The absolute path a Worker filename names, following Node's rules: a file
/// URL, an absolute path, or a `./` / `../` path relative to the current
/// directory.
fn worker_path(filename: f64) -> PathBuf {
    let value = JSValue::from_bits(filename.to_bits());
    if value.is_string() {
        let text = string_value_to_string(filename).unwrap_or_default();
        let path = Path::new(&text);
        if path.is_absolute() {
            return normalize(path);
        }
        if text.starts_with("./") || text.starts_with("../") {
            let cwd = std::env::current_dir().unwrap_or_default();
            return normalize(&cwd.join(path));
        }
        throw_with_code(
            format!(
                "The worker script or module filename must be an absolute path or a relative \
                 path starting with './' or '../'. Received \"{text}\""
            ),
            "ERR_WORKER_PATH",
            1,
        );
    }
    if value.is_pointer() {
        let href = get_object_field_from_value(filename, "href");
        if let Some(href) = string_value_to_string(href) {
            if href.starts_with("data:") {
                throw_with_code(
                    "worker_threads: a data: URL Worker is not supported in a compiled binary"
                        .to_string(),
                    "ERR_WORKER_NOT_COMPILED",
                    0,
                );
            }
        }
        // Throws ERR_INVALID_URL_SCHEME for a URL that is not file: or data:.
        let path = unsafe {
            js_url_file_url_to_path(filename, f64::from_bits(JSValue::undefined().bits()))
        };
        let text = string_value_to_string(path).unwrap_or_default();
        return normalize(Path::new(&text));
    }
    throw_with_code(
        "The \"filename\" argument must be of type string or an instance of URL".to_string(),
        "ERR_INVALID_ARG_TYPE",
        1,
    )
}

fn lookup(path: &Path) -> Option<usize> {
    let entries = WORKER_ENTRIES.read().unwrap();
    if let Some(init) = entries.get(path) {
        return Some(*init);
    }
    // The compiler registers canonical paths too, so a symlinked spelling
    // still finds its entry.
    let real = std::fs::canonicalize(path).ok()?;
    entries.get(&real).copied()
}

/// Remove `.` and resolve `..` without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `kind`: 0 = Error, 1 = TypeError.
fn throw_with_code(message: String, code: &str, kind: i32) -> ! {
    unsafe {
        perry_runtime::error::js_throw_error_with_code(
            message.as_ptr(),
            message.len(),
            code.as_ptr(),
            code.len(),
            kind,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use std::path::{Path, PathBuf};

    #[test]
    fn normalize_removes_dot_parts() {
        assert_eq!(
            normalize(Path::new("/a/./b/../c/w.ts")),
            PathBuf::from("/a/c/w.ts")
        );
    }
}
