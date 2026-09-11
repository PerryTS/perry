//! Embedded-asset registry for standalone executables (#5731).
//!
//! `perry compile --embed "./dist/**"` (or `perry.embed` in package.json /
//! `[compile] embed` in perry.toml) bakes the matched files' bytes into the
//! binary. The compiler emits a generated C object whose `constructor` calls
//! [`js_register_embedded_asset`] once per file before `main` runs, populating
//! a process-global registry. The bytes themselves live in the binary's
//! read-only data (static C literals), so the registry only stores
//! `&'static [u8]` slices into them — no copy, no per-asset heap allocation.
//!
//! Three consumers read the registry at runtime:
//!   * `fs.readFileSync` / `fs.readFile` — a `$perryfs/...` virtual path (or a
//!     bare key that matches an embedded asset) resolves to the embedded bytes
//!     instead of touching disk (see `crate::fs::read_file_bytes_with_options`).
//!   * `import { embeddedFiles } from "perry"` — [`js_perry_embedded_files`]
//!     builds the introspection array (`{ name, size, type }` per asset).
//!   * `import { readEmbedded } from "perry"` — [`js_perry_read_embedded`]
//!     returns the bytes as a `Buffer`.
//!
//! The global never frees (matching Perry's "embedded data lives for the life of
//! the process" model), mirroring the `crate::shared_sab` registry pattern.

#[cfg(feature = "bun-cli-utils")]
mod compressed;
#[cfg(feature = "bun-cli-utils")]
pub use compressed::js_register_embedded_zstd_asset;

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::string::js_string_from_bytes;
use crate::value::{js_nanbox_pointer, JSValue, TAG_TRUE};

/// Virtual-path prefix that marks a path as an embedded asset, mirroring
/// Bun's `$bunfs/`. `fs` and `readEmbedded` strip it before lookup; the
/// import-attribute lowering hands user code a `$perryfs/<name>` string.
pub const VIRTUAL_PREFIX: &str = "$perryfs/";
/// Bun standalone executables expose extracted files through this absolute
/// virtual prefix. Perry retains the full path as the registry key so user
/// code can keep passing the original string to `node:fs` and `Bun.file()`.
pub const BUNFS_ROOT_PREFIX: &str = "/$bunfs/root/";

/// One embedded file. `bytes` points into the binary's read-only data and is
/// valid for the life of the process.
struct EmbeddedAsset {
    /// Registry key — the embed-relative path, e.g. `dist/index.html`.
    name: String,
    bytes: &'static [u8],
    /// Explicit loader metadata, never inferred from the file extension.
    text_module: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EmbeddedMetadata {
    pub(crate) is_file: bool,
    pub(crate) is_directory: bool,
    pub(crate) size: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EmbeddedDirEntry {
    pub(crate) relative_path: String,
    pub(crate) name: String,
    pub(crate) parent_path: String,
    pub(crate) is_directory: bool,
}

static EMBEDDED_ASSETS: OnceLock<Mutex<Vec<EmbeddedAsset>>> = OnceLock::new();

fn registry() -> &'static Mutex<Vec<EmbeddedAsset>> {
    EMBEDDED_ASSETS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Strip the `$perryfs/` prefix (and a single leading `./`) so `$perryfs/x`,
/// `./x`, and `x` all resolve to the same registry key. Backslashes are folded
/// to `/` first so Windows-style inputs (`$perryfs\dist\index.html`,
/// `dist\index.html`) match the always-`/`-joined registry keys.
fn normalize_key(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let p = unified.strip_prefix(VIRTUAL_PREFIX).unwrap_or(&unified);
    p.strip_prefix("./").unwrap_or(p).to_string()
}

fn perry_virtual_key(path: &str) -> Option<(String, String)> {
    let unified = path.replace('\\', "/");
    if unified == "$perryfs" || unified == VIRTUAL_PREFIX {
        return Some((String::new(), "$perryfs".to_string()));
    }
    let key = unified.strip_prefix(VIRTUAL_PREFIX)?.trim_matches('/');
    Some((key.to_string(), format!("$perryfs/{key}")))
}

/// Register an embedded asset. Called once per file from the generated
/// `__attribute__((constructor))` before the runtime starts. Both `name_ptr`
/// and `bytes_ptr` point at static literals in the binary, so the recorded
/// slices are `'static`.
///
/// # Safety
/// `name_ptr`/`bytes_ptr` must point at valid, immortal byte ranges of the
/// given lengths (they always do — the compiler emits binary `.rodata`).
#[no_mangle]
pub unsafe extern "C" fn js_register_embedded_asset(
    name_ptr: *const u8,
    name_len: usize,
    bytes_ptr: *const u8,
    bytes_len: usize,
) {
    register_asset(name_ptr, name_len, bytes_ptr, bytes_len, false);
}

/// Register a Bun text-loader module as both readable bytes and a require-able
/// string. Its extension may be `.md`, `.html`, or anything else; the compiler
/// calls this only when extraction metadata explicitly declares the text loader.
///
/// # Safety
/// Same immortal byte-range contract as [`js_register_embedded_asset`].
#[no_mangle]
pub unsafe extern "C" fn js_register_embedded_text_asset(
    name_ptr: *const u8,
    name_len: usize,
    bytes_ptr: *const u8,
    bytes_len: usize,
) {
    register_asset(name_ptr, name_len, bytes_ptr, bytes_len, true);
}

unsafe fn register_asset(
    name_ptr: *const u8,
    name_len: usize,
    bytes_ptr: *const u8,
    bytes_len: usize,
    text_module: bool,
) {
    if name_ptr.is_null() || (bytes_ptr.is_null() && bytes_len != 0) {
        return;
    }
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
    let name = String::from_utf8_lossy(name_bytes).into_owned();
    let bytes: &'static [u8] = if bytes_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(bytes_ptr, bytes_len)
    };
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(EmbeddedAsset {
            name,
            bytes,
            text_module,
        });
}

/// Look up an embedded asset's bytes by virtual path (`$perryfs/...`) or by its
/// embed-relative key. Returns the `'static` slice into the binary. This is the
/// authoritative presence test — a path is "embedded" iff this returns `Some`.
pub fn lookup(path: &str) -> Option<&'static [u8]> {
    let key = normalize_key(path);
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.iter().find(|a| a.name == key).map(|a| a.bytes)
}

/// Data modules supported by `require` must opt in via their original loader.
/// Merely embedding a JS source, native addon or arbitrary file is not enough.
pub(crate) fn lookup_text_module(path: &str) -> Option<&'static [u8]> {
    let key = normalize_key(path);
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.iter()
        .find(|a| a.name == key && a.text_module)
        .map(|a| a.bytes)
}

/// Metadata for an embedded file or an inferred `$perryfs` directory.
/// Directories are implicit: every prefix before a registered asset exists.
pub(crate) fn metadata(path: &str) -> Option<EmbeddedMetadata> {
    let key = normalize_key(path);
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(asset) = reg.iter().find(|asset| asset.name == key) {
        return Some(EmbeddedMetadata {
            is_file: true,
            is_directory: false,
            size: asset.bytes.len(),
        });
    }
    let (directory_key, _) = perry_virtual_key(path)?;
    let prefix = if directory_key.is_empty() {
        String::new()
    } else {
        format!("{directory_key}/")
    };
    reg.iter()
        .any(|asset| asset.name.starts_with(&prefix))
        .then_some(EmbeddedMetadata {
            is_file: false,
            is_directory: true,
            size: 0,
        })
}

/// Sorted children of an inferred `$perryfs` directory. Recursive entries use
/// paths relative to the requested directory, matching Node's string result;
/// `name` and `parent_path` retain the pieces needed to build `fs.Dirent`.
pub(crate) fn read_dir(path: &str, recursive: bool) -> Option<Vec<EmbeddedDirEntry>> {
    let (directory_key, display_path) = perry_virtual_key(path)?;
    let prefix = if directory_key.is_empty() {
        String::new()
    } else {
        format!("{directory_key}/")
    };
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    let mut paths = BTreeMap::<String, bool>::new();
    for asset in reg.iter() {
        let Some(remainder) = asset.name.strip_prefix(&prefix) else {
            continue;
        };
        if remainder.is_empty() {
            continue;
        }
        let parts = remainder.split('/').collect::<Vec<_>>();
        let end = if recursive { parts.len() } else { 1 };
        for index in 1..=end {
            let relative_path = parts[..index].join("/");
            let is_directory = index < parts.len();
            paths
                .entry(relative_path)
                .and_modify(|known_directory| *known_directory |= is_directory)
                .or_insert(is_directory);
            if !recursive {
                break;
            }
        }
    }
    if paths.is_empty() {
        return None;
    }
    Some(
        paths
            .into_iter()
            .map(|(relative_path, is_directory)| {
                let (parent_relative, name) = relative_path
                    .rsplit_once('/')
                    .unwrap_or(("", relative_path.as_str()));
                let parent_path = if parent_relative.is_empty() {
                    display_path.clone()
                } else {
                    format!("{display_path}/{parent_relative}")
                };
                let name = name.to_string();
                EmbeddedDirEntry {
                    relative_path,
                    name,
                    parent_path,
                    is_directory,
                }
            })
            .collect(),
    )
}

/// True if `path` is an embedded-asset virtual path (carries the `$perryfs/`
/// or `/$bunfs/root/` prefix), independent of whether it actually resolves.
/// `fs` uses this to treat an unresolved virtual path as missing rather than
/// attempting a real disk read of the literal string. Actual presence is
/// [`lookup`].
pub fn is_virtual_path(path: &str) -> bool {
    let unified = path.replace('\\', "/");
    unified == "$perryfs"
        || unified.starts_with(VIRTUAL_PREFIX)
        || unified.starts_with(BUNFS_ROOT_PREFIX)
}

/// Snapshot of `(name, size)` for every embedded asset, in registration order.
fn snapshot() -> Vec<(String, usize)> {
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.iter()
        .map(|a| (a.name.clone(), a.bytes.len()))
        .collect()
}

/// Best-effort MIME type from a file extension, covering the asset classes a
/// static file server commonly emits. Defaults to `application/octet-stream`.
fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        _ => "application/octet-stream",
    }
}

fn string_value(value: &str) -> f64 {
    // Must carry STRING_TAG (not POINTER_TAG) so `typeof`/`console.log`/property
    // reads see a string, not an opaque object.
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

/// `import { embeddedFiles } from "perry"`. Returns a fresh array with one
/// `{ name, size, type }` object per embedded asset. Assets are registered (and
/// therefore listed) sorted by their embed-relative path — deterministic across
/// builds, after de-duplication.
///
/// Exposed as a (zero-arg) function rather than a bare value: member calls on a
/// native-module *value* binding (`embeddedFiles.map(...)`) are lowered as a
/// namespace dispatch (`perry.map`), so a callable that returns a real array —
/// on which normal array methods then dispatch — is the robust shape. Returns
/// the raw `*mut ArrayHeader`; the native dispatch layer NaN-boxes it (NR_PTR).
#[no_mangle]
pub extern "C" fn js_perry_embedded_files() -> *mut crate::array::ArrayHeader {
    let assets = snapshot();
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = crate::array::js_array_alloc_with_length(assets.len() as u32);
    let arr_handle = scope.root_raw_mut_ptr(arr);

    for (i, (name, size)) in assets.iter().enumerate() {
        // Root the per-asset object across the string allocations below, then
        // splice it into the already-rooted array (which makes it reachable).
        let obj = js_object_alloc(0, 3);
        let obj_handle = scope.root_raw_mut_ptr(obj);

        let name_h = scope.root_nanbox_f64(string_value(name));
        set_field(
            obj_handle.get_raw_mut_ptr::<ObjectHeader>(),
            "name",
            name_h.get_nanbox_f64(),
        );
        set_field(
            obj_handle.get_raw_mut_ptr::<ObjectHeader>(),
            "size",
            *size as f64,
        );
        let type_h = scope.root_nanbox_f64(string_value(mime_for(name)));
        set_field(
            obj_handle.get_raw_mut_ptr::<ObjectHeader>(),
            "type",
            type_h.get_nanbox_f64(),
        );

        let obj_value = js_nanbox_pointer(obj_handle.get_raw_mut_ptr::<ObjectHeader>() as i64);
        crate::array::js_array_set_f64(
            arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
            i as u32,
            obj_value,
        );
    }

    arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>()
}

/// `Perry.isStandaloneExecutable`. Any Perry-compiled binary is standalone
/// (there is no interpreter mode at runtime), so this is always `true`.
pub fn is_standalone_executable_value() -> f64 {
    f64::from_bits(TAG_TRUE)
}

/// Throw a catchable `Error` from `readEmbedded`. The native call's return ABI
/// (NR_PTR) NaN-boxes the raw pointer, so a null/garbage return would surface as
/// a bogus object rather than a thrown error — throwing keeps the
/// `readEmbedded(): Buffer` contract honest.
fn throw_embed_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

/// Throw for a `readEmbedded` miss — matches Node's `fs` "not found" semantics.
fn throw_embed_not_found(path: &str) -> ! {
    throw_embed_error(&format!("No embedded asset found for path: {path}"))
}

/// `import { readEmbedded } from "perry"`. Reads an embedded asset by virtual
/// path (`$perryfs/...`) or embed-relative key and returns its bytes as a
/// `Buffer`. Throws an `Error` when the asset is not found.
#[no_mangle]
pub extern "C" fn js_perry_read_embedded(path_value: f64) -> *mut crate::buffer::BufferHeader {
    let path = match unsafe { crate::fs::decode_path_value(path_value) } {
        Some(p) => p,
        None => throw_embed_not_found("<non-string path>"),
    };
    let Some(bytes) = lookup(&path) else {
        throw_embed_not_found(&path);
    };
    // `js_buffer_alloc` takes an i32 length; an asset ≥2 GiB would wrap to a
    // negative/garbage size. Reject it explicitly rather than corrupt memory.
    if bytes.len() > i32::MAX as usize {
        throw_embed_error(&format!(
            "Embedded asset too large to read into a Buffer ({} bytes): {path}",
            bytes.len()
        ));
    }
    unsafe {
        let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
        if !buf.is_null() {
            let buf_data = (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_data, bytes.len());
            (*buf).length = bytes.len() as u32;
        }
        buf
    }
}

// Keep the FFI symbols external under the thin-LTO + `strip=true` release
// profile. A `#[no_mangle] pub extern "C"` alone is internalized and
// dead-stripped; only individual `#[used]` typed fn-pointer statics survive
// (see the note in `typed_feedback/trace.rs`). `js_register_embedded_asset` is
// called only from the generated C constructor, and `js_perry_read_embedded`
// only from codegen-emitted callsites — both are invisible to Rust's reachability.
#[rustfmt::skip]
#[cfg(feature = "keepalive-anchors")]
mod keep_embedded {
    use super::*;
    #[cfg(feature = "keepalive-anchors")]
#[used] static K0: unsafe extern "C" fn(*const u8, usize, *const u8, usize) = js_register_embedded_asset;
    #[cfg(feature = "keepalive-anchors")]
#[used] static K1: extern "C" fn(f64) -> *mut crate::buffer::BufferHeader = js_perry_read_embedded;
    #[cfg(feature = "keepalive-anchors")]
#[used] static K2: extern "C" fn() -> *mut crate::array::ArrayHeader = js_perry_embedded_files;
    #[cfg(feature = "keepalive-anchors")]
#[used] static K3: unsafe extern "C" fn(*const u8, usize, *const u8, usize) = js_register_embedded_text_asset;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_virtual_prefix_and_dot_slash() {
        assert_eq!(normalize_key("$perryfs/dist/index.html"), "dist/index.html");
        assert_eq!(normalize_key("./dist/index.html"), "dist/index.html");
        assert_eq!(normalize_key("dist/index.html"), "dist/index.html");
        // Windows-style separators fold to `/` (before and after the prefix).
        assert_eq!(normalize_key("dist\\index.html"), "dist/index.html");
        assert_eq!(
            normalize_key("$perryfs\\dist\\index.html"),
            "dist/index.html"
        );
    }

    #[test]
    fn register_and_lookup_by_both_paths() {
        const NAME: &[u8] = b"embed-test/asset.txt";
        const DATA: &[u8] = b"embedded-bytes";
        const NESTED_NAME: &[u8] = b"embed-test/nested/two.bin";
        unsafe {
            js_register_embedded_asset(NAME.as_ptr(), NAME.len(), DATA.as_ptr(), DATA.len());
            js_register_embedded_asset(
                NESTED_NAME.as_ptr(),
                NESTED_NAME.len(),
                DATA.as_ptr(),
                DATA.len(),
            );
        }
        // Found by bare key, by `$perryfs/` virtual path, and via backslashes.
        assert_eq!(lookup("embed-test/asset.txt"), Some(DATA));
        assert_eq!(lookup("$perryfs/embed-test/asset.txt"), Some(DATA));
        assert_eq!(lookup("$perryfs\\embed-test\\asset.txt"), Some(DATA));
        // `is_virtual_path` is a pure prefix test; presence is `lookup`.
        assert!(is_virtual_path("$perryfs/anything"));
        assert!(is_virtual_path("$perryfs"));
        assert!(is_virtual_path("/$bunfs/root/assets/help.zst"));
        assert!(!is_virtual_path("not/registered.txt"));
        assert!(lookup("not/registered.txt").is_none());
        assert!(lookup("$perryfs/not-registered").is_none());

        assert_eq!(
            metadata("$perryfs/embed-test/asset.txt"),
            Some(EmbeddedMetadata {
                is_file: true,
                is_directory: false,
                size: DATA.len(),
            })
        );
        assert_eq!(
            metadata("$perryfs/embed-test/nested"),
            Some(EmbeddedMetadata {
                is_file: false,
                is_directory: true,
                size: 0,
            })
        );
        assert_eq!(
            metadata("$perryfs"),
            Some(EmbeddedMetadata {
                is_file: false,
                is_directory: true,
                size: 0,
            })
        );
        let entries = read_dir("$perryfs/embed-test", false).expect("virtual directory exists");
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.name.as_str(), entry.is_directory))
                .collect::<Vec<_>>(),
            vec![("asset.txt", false), ("nested", true)]
        );
        let recursive = read_dir("$perryfs/embed-test", true).expect("virtual directory exists");
        assert_eq!(
            recursive
                .iter()
                .map(|entry| entry.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["asset.txt", "nested", "nested/two.bin"]
        );
    }

    #[test]
    fn mime_table_covers_common_web_assets() {
        assert_eq!(mime_for("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("app.JS"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("logo.png"), "image/png");
        assert_eq!(mime_for("font.woff2"), "font/woff2");
        assert_eq!(mime_for("data.bin"), "application/octet-stream");
        assert_eq!(mime_for("noext"), "application/octet-stream");
    }

    #[test]
    fn text_modules_require_explicit_registration_and_allow_empty_bytes() {
        const TEXT: &[u8] = b"/$bunfs/root/embedded-unit-test-text.md";
        const FILE: &[u8] = b"/$bunfs/root/embedded-unit-test-file.md";
        unsafe {
            js_register_embedded_text_asset(TEXT.as_ptr(), TEXT.len(), std::ptr::null(), 0);
            js_register_embedded_asset(FILE.as_ptr(), FILE.len(), std::ptr::null(), 0);
        }
        let text = std::str::from_utf8(TEXT).unwrap();
        let file = std::str::from_utf8(FILE).unwrap();
        assert_eq!(lookup(text), Some(&[][..]));
        assert_eq!(lookup(file), Some(&[][..]));
        assert_eq!(lookup_text_module(text), Some(&[][..]));
        assert_eq!(lookup_text_module(file), None);
    }
}
