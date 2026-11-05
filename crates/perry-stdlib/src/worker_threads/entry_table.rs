//! The worker entry table: which worker files were compiled into this binary.
//!
//! The compiler gives every worker module it can name an init function
//! (`<module>__init`). The entry module's `main` registers each one here under
//! its absolute paths before any module runs. A Worker whose filename the
//! compiler could not match at the call site (`new ns.Worker(url)` on a
//! `getBuiltinModule("node:worker_threads")` namespace, a URL passed through
//! helpers, a path built at run time) is started by looking its path up in this
//! table. A path with no entry (or an eval Worker, whose code is only known at
//! run time) starts a worker whose script fails to load: like Node's
//! `MODULE_NOT_FOUND` and a Web Worker's failed script fetch, the failure is
//! the worker's asynchronous `'error'` event (`ERR_WORKER_NOT_COMPILED`), not
//! a throw from the constructor. A malformed filename still throws
//! synchronously, as in Node.
//!
//! The table only holds code addresses and is filled before user code runs, so
//! every thread (including a worker that starts nested workers) can read it.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, RwLock};

use perry_runtime::value::JSValue;

use super::{get_object_field_from_value, string_value_to_string};

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
    js_worker_threads_worker_new_with_source(filename, options, 0)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_new_with_source(
    filename: f64,
    options: f64,
    eval_entry: i64,
) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let filename = scope.root_nanbox_f64(filename);
    let options = scope.root_nanbox_f64(options);
    let outcome = perry_runtime::exception::catch_js_throw(|| {
        let argv =
            scope.root_nanbox_f64(super::WorkerOptions::prepare_argv(options.get_nanbox_f64()));
        let eval = get_object_field_from_value(options.get_nanbox_f64(), "eval");
        let (init, source) = if perry_runtime::value::js_is_truthy(eval) != 0 {
            if !JSValue::from_bits(filename.get_nanbox_f64().to_bits()).is_any_string() {
                let received = scope.root_nanbox_f64(get_object_field_from_value(
                    options.get_nanbox_f64(),
                    "eval",
                ));
                let inspected = scope.root_nanbox_f64(perry_runtime::builtins::js_util_inspect(
                    received.get_nanbox_f64(),
                    f64::from_bits(JSValue::undefined().bits()),
                ));
                let received =
                    string_value_to_string(inspected.get_nanbox_f64()).unwrap_or_default();
                perry_runtime::validators::throw_invalid_arg_value(
                    "options.eval",
                    "must be false when 'filename' is not a string",
                    &received,
                );
            }
            let init = if eval_entry == 0 {
                worker_eval_not_compiled as *const () as usize as i64
            } else {
                eval_entry
            };
            (init, "[worker eval]".to_string())
        } else {
            let path = worker_path(filename.get_nanbox_f64());
            let init =
                lookup(&path).unwrap_or(worker_entry_not_compiled as *const () as usize) as i64;
            (init, path.to_string_lossy().into_owned())
        };
        let state = super::WorkerOptions::from_value_with_argv(
            options.get_nanbox_f64(),
            argv.get_nanbox_f64(),
        );
        super::worker_new_prepared(init, options.get_nanbox_f64(), state, Some(source))
    });
    drop(scope);
    match outcome {
        Ok(value) => value,
        Err(error) => perry_runtime::exception::js_throw(error),
    }
}

/// The entry of a Worker whose script was not compiled into this binary: it
/// fails as the worker's first act, so the parent sees the worker's `'error'`
/// event and an exit, exactly where Node reports a script it cannot load.
extern "C" fn worker_entry_not_compiled() {
    throw_with_code(
        "worker_threads: this Worker's script was not compiled into the binary. Name the \
         worker file with new URL(\"<literal>\", import.meta.url) or a string literal so it \
         is compiled in"
            .to_string(),
        "ERR_WORKER_NOT_COMPILED",
        0,
    )
}

/// The entry of an eval Worker: its source is built at run time, and a
/// compiled binary cannot run code it did not compile.
extern "C" fn worker_eval_not_compiled() {
    throw_with_code(
        "worker_threads: an eval Worker needs source code that is known when the program \
         is compiled; this one is built at run time"
            .to_string(),
        "ERR_WORKER_EVAL_NOT_COMPILED",
        0,
    )
}

/// The absolute path a Worker filename names, following Node's rules: a file
/// URL, an absolute path, or a `./` / `../` path relative to the current
/// directory.
fn worker_path(filename: f64) -> PathBuf {
    let value = JSValue::from_bits(filename.to_bits());
    if value.is_string() {
        let text = string_value_to_string(filename).unwrap_or_default();
        let path = Path::new(&text);
        // `$perryfs/...` is the binary's own virtual file system (a
        // `{ type: "file" }` import of a script yields such a path, as Bun's
        // yields `/$bunfs/...`): an absolute name.
        if path.is_absolute() || text.starts_with("$perryfs/") {
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

#[cfg(test)]
mod source_tests {
    use super::*;

    extern "C" {
        fn js_shape_run_static_seed();
    }

    #[test]
    fn eval_rejects_non_string_before_worker_creation() {
        unsafe { js_shape_run_static_seed() };
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let opts = scope.root_nanbox_f64(super::super::object_value(
            perry_runtime::object::js_object_alloc(0, 0),
        ));
        let key = perry_runtime::string::js_string_from_bytes(b"eval".as_ptr(), 4);
        perry_runtime::object::js_object_set_field_by_name(
            perry_runtime::value::js_nanbox_get_pointer(opts.get_nanbox_f64()) as *mut _,
            key,
            super::super::js_bool(true),
        );
        let before = super::super::NEXT_WORKER_ID.load(std::sync::atomic::Ordering::Relaxed);
        let result = perry_runtime::exception::catch_js_throw(|| {
            js_worker_threads_worker_new_with_source(42.0, opts.get_nanbox_f64(), 0)
        });
        let error =
            scope.root_nanbox_f64(result.expect_err("non-string eval filename must reject"));
        let message = get_object_field_from_value(error.get_nanbox_f64(), "message");
        assert_eq!(string_value_to_string(message).as_deref(), Some("The property 'options.eval' must be false when 'filename' is not a string. Received true"));
        let code = get_object_field_from_value(error.get_nanbox_f64(), "code");
        assert_eq!(
            string_value_to_string(code).as_deref(),
            Some("ERR_INVALID_ARG_VALUE")
        );
        assert_eq!(
            super::super::NEXT_WORKER_ID.load(std::sync::atomic::Ordering::Relaxed),
            before
        );
    }
}
