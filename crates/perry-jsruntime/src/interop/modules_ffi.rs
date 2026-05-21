//! Module-loading FFI: `js_load_module`, `js_get_export`,
//! `js_should_use_runtime`, plus the `native_module_js_property_loader`
//! callback that perry-runtime calls when a native-module property
//! must fall back to V8.

use super::*;

use deno_core::v8;
use std::collections::hash_map::DefaultHasher;
use std::ffi::{c_char, CStr};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// V8 fallback for native module property access (e.g., ethers.Contract).
/// Loads the module via V8, finds the property, and returns a JS handle.
pub(crate) unsafe extern "C" fn native_module_js_property_loader(
    module_name_ptr: *const u8,
    module_name_len: usize,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::NativeModulePropertyLoad);
    let module_name =
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(module_name_ptr, module_name_len));
    let property_name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(
        property_name_ptr,
        property_name_len,
    ));

    // Load the module via V8
    let module_handle = js_load_module(module_name.as_ptr() as *const i8, module_name.len());
    if module_handle == 0 {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    }

    // Try getting the property as a direct named export (e.g., Contract from ethers)
    let direct = js_get_export(
        module_handle,
        property_name.as_ptr() as *const i8,
        property_name.len(),
    );
    if direct.to_bits() != 0x7FFC_0000_0000_0001 {
        return direct;
    }

    // Try through the namespace export (e.g., ethers.Contract)
    let namespace = js_get_export(
        module_handle,
        module_name.as_ptr() as *const i8,
        module_name.len(),
    );
    if namespace.to_bits() != 0x7FFC_0000_0000_0001 {
        return js_handle_object_get_property(
            namespace,
            property_name.as_ptr() as *const i8,
            property_name.len(),
        );
    }

    f64::from_bits(0x7FFC_0000_0000_0001) // undefined
}

/// Load a JavaScript module and return a handle to it
/// Returns a module handle (u64) that can be used with js_get_export and js_call_function
/// Returns 0 on failure
#[no_mangle]
pub unsafe extern "C" fn js_load_module(path_ptr: *const i8, path_len: usize) -> u64 {
    let path_slice = if path_ptr.is_null() {
        return 0;
    } else if path_len > 0 {
        std::slice::from_raw_parts(path_ptr as *const u8, path_len)
    } else {
        // Null-terminated C string
        CStr::from_ptr(path_ptr as *const c_char).to_bytes()
    };

    let path_str = match std::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    // Use the NodeModuleLoader to resolve bare module specifiers (like "ethers")
    use deno_core::ModuleLoader;
    let loader = crate::modules::NodeModuleLoader::new();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Try to resolve the module path
    let resolved_path: PathBuf = if path_str.starts_with("./")
        || path_str.starts_with("../")
        || path_str.starts_with('/')
    {
        // Relative or absolute path - resolve directly
        let path = PathBuf::from(path_str);
        std::fs::canonicalize(&path).unwrap_or(path)
    } else {
        // Bare module specifier (like "ethers") - use node_modules resolution
        let referrer = format!("file://{}/index.js", cwd.display());
        match loader.resolve(path_str, &referrer, deno_core::ResolutionKind::Import) {
            Ok(specifier) => {
                // Top-level user imports of bare specifiers that the
                // loader couldn't find produce a `perry-missing:` stub
                // (so nested V8 graph resolution can soft-throw). At
                // the top-level entry, a real failure is correct —
                // surface it as the existing hard error.
                if specifier.scheme() == "perry-missing" {
                    eprintln!(
                            "[js_load_module] FAILED to load '{}': bare module not found in node_modules",
                            path_str
                        );
                    return 0;
                }
                specifier
                    .to_file_path()
                    .unwrap_or_else(|_| PathBuf::from(path_str))
            }
            Err(e) => {
                log::error!("Failed to resolve module '{}': {}", path_str, e);
                return 0;
            }
        }
    };

    let canonical = resolved_path.clone();

    let target_specifier = match deno_core::ModuleSpecifier::from_file_path(&canonical) {
        Ok(s) => s,
        Err(_) => {
            log::error!(
                "Failed to create module specifier from path: {:?}",
                canonical
            );
            return 0;
        }
    };
    let target_specifier_str = target_specifier.to_string();
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    // Materialize the proxy in a per-process temp directory rather than the
    // user's CWD. Deno's recursive loader still resolves the proxy specifier
    // through our NodeModuleLoader, so the file must exist on disk even
    // though the source is also supplied via load_side_es_module_from_code.
    let proxy_dir = std::env::temp_dir().join(format!("perry-js-proxy-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&proxy_dir);
    let proxy_path = proxy_dir.join(format!("__perry_js_proxy_{:016x}.mjs", hasher.finish()));
    let specifier = match deno_core::ModuleSpecifier::from_file_path(&proxy_path) {
        Ok(s) => s,
        Err(_) => {
            log::error!(
                "Failed to create proxy module specifier for {:?}",
                canonical
            );
            return 0;
        }
    };
    let proxy_code = format!(
        r#"import * as __perry_ns from {target:?};
const __perry_default = Object.prototype.hasOwnProperty.call(__perry_ns, "default") ? __perry_ns.default : __perry_ns;
export {{ __perry_default as default }};
export * from {target:?};
"#,
        target = target_specifier_str
    );
    if let Ok(proxy_file_path) = specifier.to_file_path() {
        let _ = std::fs::write(proxy_file_path, &proxy_code);
    }

    let tokio_rt = get_tokio_runtime();

    let result = tokio_rt.block_on(async {
        JS_RUNTIME.with(|cell| {
            let mut opt = cell.borrow_mut();
            let state = match opt.as_mut() {
                Some(s) => s,
                None => {
                    eprintln!("[js_load_module] no JS runtime state!");
                    return Err(());
                }
            };

            // Check if already loaded
            if let Some(&module_id) = state.loaded_modules.get(&canonical) {
                return Ok(module_id as u64);
            }
            bump_v8_entry(V8EntryKind::ModuleLoad);

            // Use a dedicated current-thread Tokio runtime to avoid thread pool starvation deadlock.
            tokio::task::block_in_place(|| {
                let local_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create local Tokio runtime for module loading");
                local_rt.block_on(async {
                    // Load a proxy module rather than the target directly. The target may
                    // already have been evaluated as a dependency of another JS module; a
                    // proxy imports and re-exports it without evaluating that target as a
                    // new side root.
                    let module_id = match state
                        .runtime
                        .load_side_es_module_from_code(&specifier, proxy_code)
                        .await
                    {
                        Ok(id) => id,
                        Err(e) => {
                            eprintln!("[js_load_module] FAILED to load '{}': {}", path_str, e);
                            return Err(());
                        }
                    };

                    // Start evaluation, but let Perry's main event loop drive
                    // the returned future via js_run_jsruntime_pump().
                    let eval_future = state.runtime.mod_evaluate(module_id);
                    state.pending_module_evaluations.insert(
                        module_id,
                        crate::PendingModuleEvaluation {
                            canonical_path: canonical.clone(),
                            future: Box::pin(eval_future),
                        },
                    );
                    bump_jsruntime(&JSRUNTIME_MODULE_EVALS_STARTED);

                    // Cache the module immediately so repeated imports reuse
                    // the same module id while evaluation is pump-driven.
                    state.loaded_modules.insert(canonical.clone(), module_id);
                    perry_runtime::event_pump::js_notify_main_thread();
                    let _ = poll_pending_module_evaluations(state);

                    Ok(module_id as u64)
                })
            })
        })
    });

    result.unwrap_or(0)
}

/// Get an export from a loaded module
/// Returns the value as a NaN-boxed f64
#[no_mangle]
pub unsafe extern "C" fn js_get_export(
    module_handle: u64,
    export_name_ptr: *const i8,
    export_name_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::ExportGet);
    let name_slice = if export_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    } else if export_name_len > 0 {
        std::slice::from_raw_parts(export_name_ptr as *const u8, export_name_len)
    } else {
        CStr::from_ptr(export_name_ptr as *const c_char).to_bytes()
    };

    let export_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    with_runtime(|state| {
        let module_id = module_handle as deno_core::ModuleId;
        let namespace = match state.runtime.get_module_namespace(module_id) {
            Ok(ns) => ns,
            Err(e) => {
                eprintln!("[js_get_export] failed to get namespace: {}", e);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        deno_core::scope!(scope, &mut state.runtime);
        let namespace = v8::Local::new(scope, namespace);

        // For namespace imports (export_name == "*"), return the entire module namespace object
        if export_name == "*" {
            let result = v8_to_native(scope, namespace.into());
            return result;
        }

        let key = match v8::String::new(scope, export_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        let value = match namespace.get(scope, key.into()) {
            Some(v) => v,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        v8_to_native_export_value(scope, value)
    })
}

/// Check if a module path should be loaded via the JS runtime
/// Returns 1 if it should use JS runtime, 0 if it should be compiled natively
#[no_mangle]
pub unsafe extern "C" fn js_should_use_runtime(path_ptr: *const i8, path_len: usize) -> i32 {
    bump_v8_entry(V8EntryKind::ShouldUseRuntime);
    let path_slice = if path_ptr.is_null() {
        return 0;
    } else if path_len > 0 {
        std::slice::from_raw_parts(path_ptr as *const u8, path_len)
    } else {
        CStr::from_ptr(path_ptr as *const c_char).to_bytes()
    };

    let path_str = match std::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    // Check if this is a .js file (not .ts/.tsx)
    if path_str.ends_with(".js") || path_str.ends_with(".mjs") || path_str.ends_with(".cjs") {
        return 1;
    }

    // Check if this is in node_modules and not TypeScript
    if path_str.contains("node_modules") {
        let path = PathBuf::from(path_str);

        // If it's a directory reference, check for TypeScript files
        if path.is_dir() {
            let has_ts = path.join("index.ts").exists()
                || path.join("index.tsx").exists()
                || path.join("src/index.ts").exists();

            if !has_ts {
                return 1;
            }
        }
    }

    0
}

pub(crate) fn c_str_to_utf8(ptr: *const i8, len: usize) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe {
        if len > 0 {
            std::slice::from_raw_parts(ptr as *const u8, len)
        } else {
            CStr::from_ptr(ptr as *const c_char).to_bytes()
        }
    };
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}
