//! Dynamic constants are reached through the installer of their owning module.
//! A getter specializes the shared resolver by literal namespace name, so its
//! function pointer retains only that module's arms and tables. The optimizer
//! barrier on stores prevents LLVM from reintroducing a direct reader edge.
use super::constants::selected_normalized_constant;
use std::sync::atomic::{AtomicPtr, Ordering};

type Getter = unsafe fn(&str, &str, f64, bool) -> Option<f64>;

macro_rules! getters {
    ($( $install:ident, $slot:ident, [$($name:literal),+]; )+) => {
        $(static $slot: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
        pub(crate) fn $install() {
            unsafe fn resolve(module: &str, property: &str, namespace: f64, is_default: bool) -> Option<f64> {
                match module {
                    $($name => selected_normalized_constant(
                        $name, property, namespace, is_default),)+
                    _ => None,
                }
            }
            $slot.store(std::hint::black_box(resolve as Getter) as *mut (), Ordering::Release);
        })+

        pub(crate) fn install_all() { $($install();)+ }

        unsafe fn installed(module: &str, property: &str, namespace: f64, is_default: bool) -> Option<f64> {
            match module {
                $($($name)|+ => {
                    let pointer = $slot.load(Ordering::Acquire);
                    #[cfg(test)]
                    let pointer = if pointer.is_null()
                        && !super::super::native_module_registry::NM_TEST_DISABLE_LAZY_INSTALL
                            .load(Ordering::Relaxed)
                    {
                        super::super::native_module_registry::js_nm_install_all();
                        $slot.load(Ordering::Acquire)
                    } else { pointer };
                    if pointer.is_null() { return None; }
                    let getter = std::mem::transmute::<*mut (), Getter>(pointer);
                    getter(module, property, namespace, is_default)
                },)+
                _ => None,
            }
        }
    }
}

getters! {
    assert, ASSERT, ["assert", "assert/strict"];
    async_hooks, ASYNC_HOOKS, ["async_hooks"];
    buffer, BUFFER, ["buffer", "buffer.Buffer", "buffer.constants"];
    bun_ffi, BUN_FFI, ["bun:ffi", "ffi"];
    crypto, CRYPTO, ["crypto", "crypto.constants"];
    webcrypto, WEBCRYPTO, ["crypto.webcrypto", "crypto.subtle"];
    dns, DNS, ["dns", "dns/promises"];
    domain, DOMAIN, ["domain"];
    events, EVENTS, ["events"];
    fs, FS, ["fs", "fs.constants"];
    module, MODULE, ["module"];
    net, NET, ["net"];
    os, OS, ["os", "os.constants", "os.constants.signals", "os.constants.errno", "os.constants.priority", "os.constants.dlopen"];
    path, PATH, ["path", "path.posix", "path.win32"];
    perf, PERF, ["perf_hooks", "perf_hooks.constants", "perf_histogram"];
    punycode, PUNYCODE, ["punycode"];
    querystring, QUERYSTRING, ["querystring"];
    readline, READLINE, ["readline"];
    repl, REPL, ["repl"];
    sqlite, SQLITE, ["sqlite", "sqlite.constants"];
    stream, STREAM, ["stream"];
    timers, TIMERS, ["timers"];
    url, URL, ["url"];
    util, UTIL, ["util"];
    v8, V8, ["v8"];
    vm, VM, ["vm"];
    wasi, WASI, ["wasi"];
    worker_threads, WORKER_THREADS, ["worker_threads"];
    constants, CONSTANTS, ["constants"];
}

pub(super) unsafe fn get(module: &str, property: &str, namespace: f64) -> Option<f64> {
    let base = super::cjs_default_base_module(module);
    let is_default = base.is_some();
    let module = if module == "wasi.default" {
        "wasi"
    } else {
        base.unwrap_or(module)
    };
    // Modules with no constants still have a CJS default export.
    if property == "default" && !is_default && module != "process" {
        if let Some(value) = super::cjs_default_export_value(module) {
            return Some(value);
        }
    }
    macro_rules! selected {
        ($name:literal) => {
            selected_normalized_constant($name, property, namespace, is_default)
        };
    }
    // These modules already select their value providers through other slots.
    match module {
        "process" => selected!("process"),
        "process.namespace" => selected!("process.namespace"),
        "inspector" => selected!("inspector"),
        "inspector/promises" => selected!("inspector/promises"),
        "tls" => selected!("tls"),
        "zlib" => selected!("zlib"),
        "zlib.constants" => selected!("zlib.constants"),
        "http" => selected!("http"),
        "https" => selected!("https"),
        "http2" => selected!("http2"),
        "http2.constants" => selected!("http2.constants"),
        "cluster" => selected!("cluster"),
        "test" => selected!("test"),
        "timers/promises" => selected!("timers/promises"),
        _ => installed(module, property, namespace, is_default),
    }
}

#[cfg(test)]
mod tests {
    use super::super::constants::selected_native_module_constant;
    use super::*;

    #[test]
    fn constant_getter_requires_its_install_and_covers_subnamespaces() {
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                super::super::super::native_module_registry::NM_TEST_DISABLE_LAZY_INSTALL
                    .store(false, Ordering::Relaxed);
            }
        }
        super::super::super::native_module_registry::NM_TEST_DISABLE_LAZY_INSTALL
            .store(true, Ordering::Relaxed);
        let _restore = Restore;
        for slot in [&FS, &OS, &CRYPTO, &CONSTANTS] {
            slot.store(std::ptr::null_mut(), Ordering::Release);
        }
        unsafe {
            assert!(get("fs.constants", "F_OK", 0.0).is_none());
            assert!(get("os.constants.errno", "EACCES", 0.0).is_none());
            assert!(get("crypto.constants", "RSA_PKCS1_PADDING", 0.0).is_none());
            assert!(get("constants.default", "R_OK", 0.0).is_none());
        }
        fs();
        os();
        crypto();
        constants();
        for (module, property) in [
            ("fs", "F_OK"),
            ("constants.default", "R_OK"),
            ("fs.constants", "O_RDONLY"),
            ("os.constants.errno", "EACCES"),
            ("os.constants.signals", "SIGTERM"),
            ("crypto.constants", "RSA_PKCS1_PADDING"),
        ] {
            unsafe {
                let expected = selected_native_module_constant(module, property, 0.0)
                    .expect("fixture must name a numeric constant");
                let actual = get(module, property, 0.0).expect("installed getter missing");
                assert_eq!(actual.to_bits(), expected.to_bits(), "{module}.{property}");
            }
        }
    }
}

/// Selected when compiled code exposes a loader capability to computed reads.
#[no_mangle]
pub extern "C" fn js_nm_install_all_constants() {
    install_all();
}
