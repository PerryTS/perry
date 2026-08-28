use super::*;
use crate::value::JSValue;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
#[cfg(unix)]
use std::ffi::CStr;
use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};

const MANIFEST_SCHEMA: u32 = 1;
const SHIPPING_MODEL: &str = "sidecar-v1";

#[derive(Deserialize)]
struct SidecarManifest {
    schema_version: u32,
    policy_version: u32,
    napi_version: u32,
    shipping_model: String,
    target: String,
    addons: Vec<ManifestAddon>,
}

#[derive(Deserialize)]
struct ManifestAddon {
    logical_id: String,
    package: String,
    version: String,
    entry: String,
    files: Vec<ManifestFile>,
}

#[derive(Deserialize)]
struct ManifestFile {
    path: String,
    sha256: String,
    size: u64,
}

pub(crate) struct LoadedAddon {
    logical_id: String,
    canonical_path: PathBuf,
    handle: usize,
    pub(crate) exports_bits: u64,
}

#[repr(C)]
pub struct NapiModule {
    nm_version: i32,
    nm_flags: u32,
    nm_filename: *const c_char,
    nm_register_func: Option<unsafe extern "C" fn(NapiEnv, NapiValue) -> NapiValue>,
    nm_modname: *const c_char,
    nm_priv: *mut c_void,
    reserved: [*mut c_void; 4],
}

#[derive(Default)]
struct LegacyCapture {
    active_path: Option<PathBuf>,
    descriptor: Option<usize>,
    registrations: u32,
    outside_load: bool,
}

crate::perry_thread_local! {
    static LEGACY_CAPTURE: RefCell<LegacyCapture> = RefCell::new(LegacyCapture::default());
}

fn sidecar_root() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate the Perry executable: {error}"))?;
    let file_name = executable
        .file_name()
        .ok_or_else(|| "the Perry executable has no filename".to_string())?
        .to_string_lossy();
    if executable
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "MacOS")
    {
        if let Some(contents) = executable
            .parent()
            .and_then(Path::parent)
            .filter(|path| path.file_name().is_some_and(|name| name == "Contents"))
        {
            return Ok(contents
                .join("Frameworks")
                .join(format!("{file_name}.perry-native")));
        }
    }
    Ok(executable.with_file_name(format!("{file_name}.perry-native")))
}

fn load_manifest(root: &Path) -> Result<SidecarManifest, String> {
    let path = root.join("manifest.json");
    let bytes = std::fs::read(&path).map_err(|error| {
        format!(
            "Node-API sidecar manifest {} is unavailable: {error}",
            path.display()
        )
    })?;
    let manifest: SidecarManifest = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "Node-API sidecar manifest {} is invalid: {error}",
            path.display()
        )
    })?;
    if manifest.schema_version != MANIFEST_SCHEMA
        || manifest.policy_version != MANIFEST_SCHEMA
        || manifest.shipping_model != SHIPPING_MODEL
        || manifest.napi_version != NAPI_VERSION
    {
        return Err(format!(
            "Node-API sidecar policy is incompatible (schema {}, model {}, N-API {}; Perry requires schema {}, model {}, N-API {})",
            manifest.schema_version,
            manifest.shipping_model,
            manifest.napi_version,
            MANIFEST_SCHEMA,
            SHIPPING_MODEL,
            NAPI_VERSION
        ));
    }
    if manifest.target != env!("PERRY_RUNTIME_TARGET") {
        return Err(format!(
            "Node-API sidecar target `{}` does not match this executable's `{}` target",
            manifest.target,
            env!("PERRY_RUNTIME_TARGET")
        ));
    }
    Ok(manifest)
}

fn normalize_request(request: &str) -> String {
    request
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn manifest_entry_for<'a>(
    manifest: &'a SidecarManifest,
    request: &str,
) -> Result<&'a ManifestAddon, String> {
    let request = normalize_request(request);
    if let Some(addon) = manifest
        .addons
        .iter()
        .find(|addon| addon.logical_id == request)
    {
        return Ok(addon);
    }
    if let Some((_, package_path)) = request.rsplit_once("/node_modules/") {
        if let Some(addon) = manifest
            .addons
            .iter()
            .find(|addon| addon.logical_id == package_path)
        {
            return Ok(addon);
        }
    }
    let mut suffix_matches = manifest
        .addons
        .iter()
        .filter(|addon| request.ends_with(&addon.logical_id) || request.ends_with(&addon.entry));
    let first = suffix_matches.next();
    if first.is_some() && suffix_matches.next().is_none() {
        return Ok(first.unwrap());
    }
    Err(format!(
        "Node-API addon `{request}` is not authorized by this executable's compile-time manifest"
    ))
}

fn safe_payload_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let candidate = root.join(relative);
    let root = root.canonicalize().map_err(|error| {
        format!(
            "cannot canonicalize sidecar root {}: {error}",
            root.display()
        )
    })?;
    let canonical = candidate.canonicalize().map_err(|error| {
        format!(
            "missing Node-API sidecar payload {}: {error}",
            candidate.display()
        )
    })?;
    if !canonical.starts_with(&root) {
        return Err(format!(
            "Node-API sidecar path escapes its package root: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn verify_addon_payload(root: &Path, addon: &ManifestAddon) -> Result<PathBuf, String> {
    if addon.files.is_empty() {
        return Err(format!(
            "Node-API addon `{}` has an empty payload",
            addon.logical_id
        ));
    }
    let entry = safe_payload_path(root, &addon.entry)?;
    let mut entry_verified = false;
    for file in &addon.files {
        let path = safe_payload_path(root, &file.path)?;
        let bytes = std::fs::read(&path).map_err(|error| {
            format!(
                "cannot read Node-API sidecar payload {}: {error}",
                path.display()
            )
        })?;
        if bytes.len() as u64 != file.size {
            return Err(format!(
                "Node-API sidecar payload {} has size {}, expected {}",
                path.display(),
                bytes.len(),
                file.size
            ));
        }
        let actual = hex::encode(Sha256::digest(&bytes));
        if actual != file.sha256 {
            return Err(format!(
                "Node-API sidecar payload {} failed its SHA-256 check",
                path.display()
            ));
        }
        if path == entry {
            entry_verified = true;
        }
    }
    if !entry_verified {
        return Err(format!(
            "Node-API addon `{}` entry {} is not listed among its verified payload files",
            addon.logical_id,
            entry.display()
        ));
    }
    Ok(entry)
}

#[cfg(unix)]
unsafe fn open_library(path: &Path) -> Result<usize, String> {
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "Node-API addon path contains a NUL byte".to_string())?;
    libc::dlerror();
    let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
    if handle.is_null() {
        let error = libc::dlerror();
        let detail = if error.is_null() {
            "unknown dynamic loader error".to_string()
        } else {
            CStr::from_ptr(error).to_string_lossy().into_owned()
        };
        Err(detail)
    } else {
        Ok(handle as usize)
    }
}

#[cfg(unix)]
unsafe fn find_symbol(handle: usize, name: &[u8]) -> Option<usize> {
    let name = CString::new(name).ok()?;
    let symbol = libc::dlsym(handle as *mut c_void, name.as_ptr());
    (!symbol.is_null()).then_some(symbol as usize)
}

#[cfg(unix)]
unsafe fn close_library(handle: usize) {
    libc::dlclose(handle as *mut c_void);
}

#[cfg(windows)]
unsafe fn open_library(path: &Path) -> Result<usize, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{
        LoadLibraryExW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = LoadLibraryExW(
        wide.as_ptr(),
        std::ptr::null_mut(),
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    );
    if handle.is_null() {
        Err(format!(
            "Windows loader error {}",
            windows_sys::Win32::Foundation::GetLastError()
        ))
    } else {
        Ok(handle as usize)
    }
}

#[cfg(windows)]
unsafe fn find_symbol(handle: usize, name: &[u8]) -> Option<usize> {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    let name = CString::new(name).ok()?;
    GetProcAddress(handle as *mut c_void, name.as_ptr() as *const u8).map(|symbol| symbol as usize)
}

#[cfg(windows)]
unsafe fn close_library(handle: usize) {
    windows_sys::Win32::Foundation::FreeLibrary(handle as *mut c_void);
}

#[cfg(not(any(unix, windows)))]
unsafe fn open_library(_path: &Path) -> Result<usize, String> {
    Err("Node-API addons are supported only on desktop/server targets".to_string())
}
#[cfg(not(any(unix, windows)))]
unsafe fn find_symbol(_handle: usize, _name: &[u8]) -> Option<usize> {
    None
}
#[cfg(not(any(unix, windows)))]
unsafe fn close_library(_handle: usize) {}

fn begin_legacy_capture(path: &Path) -> Result<(), String> {
    LEGACY_CAPTURE.with(|capture| {
        let mut capture = capture.borrow_mut();
        if capture.active_path.is_some() {
            return Err("nested Node-API module loading is not supported".to_string());
        }
        capture.active_path = Some(path.to_path_buf());
        capture.descriptor = None;
        capture.registrations = 0;
        capture.outside_load = false;
        Ok(())
    })
}

fn end_legacy_capture() -> Result<Option<usize>, String> {
    LEGACY_CAPTURE.with(|capture| {
        let mut capture = capture.borrow_mut();
        capture.active_path = None;
        if capture.outside_load {
            return Err("napi_module_register was called outside an active addon load".to_string());
        }
        if capture.registrations > 1 {
            return Err(
                "a Node-API addon registered more than one legacy module descriptor".to_string(),
            );
        }
        Ok(capture.descriptor.take())
    })
}

#[no_mangle]
pub unsafe extern "C" fn napi_module_register(module: *mut NapiModule) {
    LEGACY_CAPTURE.with(|capture| {
        let mut capture = capture.borrow_mut();
        if capture.active_path.is_none() || module.is_null() {
            capture.outside_load = true;
            return;
        }
        capture.registrations = capture.registrations.saturating_add(1);
        if capture.descriptor.is_none() {
            capture.descriptor = Some(module as usize);
        }
    });
}

unsafe fn initialize_addon(
    env: NapiEnv,
    handle: usize,
    legacy: Option<usize>,
) -> Result<u64, String> {
    type VersionFn = unsafe extern "C" fn() -> i32;
    type RegisterFn = unsafe extern "C" fn(NapiEnv, NapiValue) -> NapiValue;
    if let Some(version) = find_symbol(handle, b"node_api_module_get_api_version_v1") {
        let version: VersionFn = std::mem::transmute(version);
        let requested = version();
        if requested < 1 || requested as u32 > NAPI_VERSION {
            return Err(format!(
                "addon requests Node-API version {requested}, but Perry supports versions 1 through {NAPI_VERSION}"
            ));
        }
    }
    let mut exports = std::ptr::null_mut();
    let status = napi_create_object(env, &mut exports);
    if status != NapiStatus::Ok {
        return Err("could not create the addon exports object".to_string());
    }
    let register: RegisterFn =
        if let Some(register) = find_symbol(handle, b"napi_register_module_v1") {
            std::mem::transmute(register)
        } else if let Some(legacy) = legacy {
            let descriptor = &*(legacy as *const NapiModule);
            if descriptor.nm_version != 1 {
                return Err(format!(
                    "unsupported legacy Node module ABI version {}",
                    descriptor.nm_version
                ));
            }
            descriptor
                .nm_register_func
                .ok_or_else(|| "legacy Node-API descriptor has no initializer".to_string())?
        } else {
            return Err(
                "addon exports neither napi_register_module_v1 nor a legacy napi_module descriptor"
                    .to_string(),
            );
        };
    let returned = register(env, exports);
    if pending_exception(env).is_some() {
        return Err("addon initializer left a pending JavaScript exception".to_string());
    }
    let selected = if returned.is_null() {
        exports
    } else {
        returned
    };
    value_bits(env, selected)
        .map_err(|_| "addon initializer returned an invalid napi_value".to_string())
}

pub fn load_addon(request: &str) -> Result<f64, String> {
    let env = current_env();
    let root = sidecar_root()?;
    let manifest = load_manifest(&root)?;
    let addon = manifest_entry_for(&manifest, request)?;
    let _identity = (&addon.package, &addon.version);
    if let Some(bits) = with_env(env, |env| {
        env.loaded_addons
            .iter()
            .find(|loaded| loaded.logical_id == addon.logical_id)
            .map(|loaded| loaded.exports_bits)
    })
    .flatten()
    {
        return Ok(f64::from_bits(bits));
    }
    let path = verify_addon_payload(&root, addon)?;
    let entered = with_env_mut(env, |env| {
        if env.currently_loading_filename.is_some() {
            return false;
        }
        env.currently_loading_filename = Some(addon.logical_id.clone());
        true
    }) == Some(true);
    if !entered {
        return Err("nested Node-API addon initialization is not supported".to_string());
    }
    let load_result = (|| {
        begin_legacy_capture(&path)?;
        // Platform dynamic loaders accept a pathname, not the bytes or a
        // portable file handle retained by verification. A local writer can
        // therefore race this open after hashing; the accepted trust boundary
        // and deployment mitigation are documented in node-api-host.md.
        let handle = match unsafe { open_library(&path) } {
            Ok(handle) => handle,
            Err(error) => {
                let _ = end_legacy_capture();
                return Err(format!(
                    "failed to load Node-API addon {}: {error}",
                    path.display()
                ));
            }
        };
        let legacy = match end_legacy_capture() {
            Ok(legacy) => legacy,
            Err(error) => {
                unsafe { close_library(handle) };
                return Err(error);
            }
        };
        match unsafe { initialize_addon(env, handle, legacy) } {
            Ok(bits) => Ok((handle, bits)),
            Err(error) => {
                unsafe { close_library(handle) };
                Err(format!(
                    "failed to initialize Node-API addon {}: {error}",
                    path.display()
                ))
            }
        }
    })();
    with_env_mut(env, |env| env.currently_loading_filename = None);
    let (handle, exports_bits) = load_result?;
    let loaded = LoadedAddon {
        logical_id: addon.logical_id.clone(),
        canonical_path: path,
        handle,
        exports_bits,
    };
    with_env_mut(env, |env| env.loaded_addons.push(loaded))
        .ok_or_else(|| "Node-API environment is unavailable".to_string())?;
    Ok(f64::from_bits(exports_bits))
}

pub(crate) fn close_loaded_addons(env: NapiEnv) {
    let addons =
        with_env_mut(env, |env| std::mem::take(&mut env.loaded_addons)).unwrap_or_default();
    for addon in addons.into_iter().rev() {
        let _ = addon.canonical_path;
        unsafe { close_library(addon.handle) };
    }
}

pub fn load_addon_or_throw(request: &str) -> f64 {
    match load_addon(request) {
        Ok(exports) => exports,
        Err(error) => crate::fs::validate::throw_error_with_code(&error, "ERR_DLOPEN_FAILED"),
    }
}

pub(crate) unsafe fn js_string_value(value: f64) -> Option<String> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_any_string() {
        return None;
    }
    let mut short = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    crate::string::js_string_key_bytes(value, &mut short)
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}
