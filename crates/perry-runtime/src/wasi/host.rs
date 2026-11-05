//! A GC-owned WASI context, retained through the import closures and their
//! instance owners. No process-global capability table or ambient context.
use super::*;
use crate::webassembly::host::*;
use std::ffi::{c_char, c_void, CStr};
const TYPE_ID: u64 = 0xa31e_b47d_d211_1021;
static VTABLE: crate::native_payload::PayloadVTable = crate::native_payload::PayloadVTable {
    drop: drop_context,
    stream: None,
};
struct Context {
    host: *mut c_void,
    config: Vec<u8>,
    stdio: [Option<std::fs::File>; 3],
}
impl Drop for Context {
    fn drop(&mut self) {
        if !self.host.is_null() {
            unsafe { perry_wasm_host_wasi_drop(self.host) };
        }
    }
}
unsafe extern "C" fn drop_context(resource: *mut c_void, _: *mut c_void) {
    drop(Box::from_raw(resource.cast::<Context>()));
}
fn host_error(error: *mut c_char) -> ! {
    let message = if error.is_null() {
        "WASI host failure".to_string()
    } else {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { perry_wasm_host_string_free(error) };
        message
    };
    crate::fs::validate::throw_error_with_code(&message, "ERR_WASI_HOST");
}
impl Context {
    fn open(&mut self) {
        if !self.host.is_null() {
            return;
        }
        let stdio = self.stdio.each_ref().map(|file| {
            let Some(file) = file else {
                return -1;
            };
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                file.as_raw_fd() as i64
            }
            #[cfg(windows)]
            {
                use std::os::windows::io::AsRawHandle;
                file.as_raw_handle() as i64
            }
            #[cfg(not(any(unix, windows)))]
            {
                let _ = file;
                -1
            }
        });
        let mut error = std::ptr::null_mut();
        self.host = unsafe {
            perry_wasm_host_wasi_new(
                self.config.as_ptr(),
                self.config.len(),
                stdio.as_ptr(),
                &mut error,
            )
        };
        if self.host.is_null() {
            host_error(error);
        }
        self.config.clear();
        self.stdio = [None, None, None];
    }
}
fn strings(value: f64) -> Vec<String> {
    let array = JSValue::from_bits(value.to_bits()).as_pointer::<crate::array::ArrayHeader>();
    (0..crate::array::js_array_length(array))
        .map(|i| {
            crate::builtins::jsvalue_string_content(crate::array::js_array_get_f64(array, i))
                .unwrap_or_default()
        })
        .collect()
}
pub(super) fn new(options: &ValidatedOptions) -> f64 {
    let config = serde_json::to_vec(&serde_json::json!({
        "args": strings(options.args), "env": strings(options.env), "preopens": options.preopens,
    }))
    .expect("WASI configuration is JSON strings");
    let stdio = options.stdio.map(|fd| {
        // Process stdio is inherited by the provider. Explicit descriptors
        // from fs.openSync are Perry IDs and must be cloned through fs.
        fd.map(|fd| {
            crate::fs_hook_slots::try_clone_registered_fd(fd).unwrap_or_else(|| {
                crate::fs::validate::throw_error_with_code(
                    "bad WASI stdio descriptor",
                    "UVWASI_EINVAL",
                )
            })
        })
    });
    let context = Box::new(Context {
        host: std::ptr::null_mut(),
        config,
        stdio,
    });
    // Shape-only programs need not link the optional engine. Opening occurs
    // here when linked, otherwise the first syscall reports the missing host.
    unsafe {
        let cell = crate::native_handle::native_handle_new_rust_payload(
            Box::into_raw(context).cast(),
            TYPE_ID,
            &VTABLE,
            "Perry.WASI.Context",
            false,
        );
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(cell as i64));
        crate::native_handle::native_handle_set_external_bytes(
            cell,
            std::mem::size_of::<Context>(),
        );
        if host_linked() {
            let cell = JSValue::from_bits(value.get_nanbox_f64().to_bits())
                .as_pointer::<crate::native_handle::NativeHandleHeader>()
                as *mut _;
            let resource = crate::native_handle::native_handle_rust_payload_ptr(cell, TYPE_ID)
                .cast::<Context>();
            (*resource).open();
        }
        value.get_nanbox_f64()
    }
}
pub(super) fn attach(import: f64, context: f64) {
    let _publication = crate::gc::GcSuppressScope::new();
    unsafe {
        crate::object::owned_state_bag::store(
            heap_object_ptr(import).expect("WASI import object") as usize,
            crate::object::owned_state_bag::WASI_CONTEXT,
            context,
        );
    }
}
pub(super) fn exit_marker(import: f64) -> f64 {
    let _publication = crate::gc::GcSuppressScope::new();
    unsafe {
        crate::object::owned_state_bag::load(
            heap_object_ptr(import).expect("WASI import object") as usize,
            crate::object::owned_state_bag::WASI_CONTEXT,
        )
    }
}
pub(crate) fn is_exit_marker(value: f64) -> bool {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return false;
    }
    let cell = value.as_pointer::<crate::native_handle::NativeHandleHeader>()
        as *mut crate::native_handle::NativeHandleHeader;
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(cell as usize)
        else {
            return false;
        };
        header.as_ref().obj_type == crate::gc::GC_TYPE_NATIVE_HANDLE
            && !crate::native_handle::native_handle_rust_payload_ptr(cell, TYPE_ID).is_null()
    }
}
fn context(import: f64) -> *mut Context {
    let value = JSValue::from_bits(exit_marker(import).to_bits());
    unsafe {
        crate::native_handle::native_handle_rust_payload_ptr(
            value.as_pointer::<crate::native_handle::NativeHandleHeader>() as *mut _,
            TYPE_ID,
        )
        .cast()
    }
}

pub(super) fn call(import: f64, syscall: usize, args: &[u64]) -> i32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let import = scope.root_nanbox_f64(import);
    if import_binding_name(import.get_nanbox_f64()) == "wasi_unstable" {
        crate::fs::validate::throw_error_with_code(
            "WASI Preview 0 execution is unsupported; use version preview1",
            "ERR_WASI_UNSUPPORTED_VERSION",
        );
    }
    ensure_import_started(import.get_nanbox_f64());
    let ctx = context(import.get_nanbox_f64());
    assert!(!ctx.is_null(), "WASI imports own a native context");
    unsafe { (*ctx).open() };
    let buffer = bound_memory(import.get_nanbox_f64());
    let mut error = std::ptr::null_mut();
    // The provider cannot call JS or collect; no guest-memory reference lives
    // across a JS allocation, and the import trampoline refreshes grown memory.
    let result = crate::buffer::bytes::no_gc(|scope| unsafe {
        let Ok(memory) =
            crate::buffer::bytes::bytes_mut(crate::value::js_nanbox_pointer(buffer as i64), scope)
        else {
            return 21;
        }; // FAULT
        perry_wasm_host_wasi_call(
            (*ctx).host,
            syscall,
            memory.as_mut_ptr(),
            memory.len(),
            args.as_ptr(),
            args.len(),
            &mut error,
        )
    });
    if result == perry_abi::wasi::IMPORT_TRAP {
        host_error(error);
    }
    if result == perry_abi::wasi::IMPORT_EXIT {
        let code = args[0] as i32;
        if !JSValue::from_bits(
            import_field(
                import.get_nanbox_f64(),
                FIELD_WASI_RETURN_ON_EXIT.as_bytes(),
            )
            .to_bits(),
        )
        .as_bool()
        {
            std::process::exit(code);
        }
        WASI_EXIT_CODE.with(|slot| slot.set(Some(code)));
    }
    result
}

/// Only genuine node:wasi closures use native syscalls. A user-supplied
/// replacement, even named wasi_snapshot_preview1.proc_exit, remains JS.
pub(crate) fn call_import(
    callback: f64,
    kinds: &[u8],
    args: &[u64],
    results: &[u8],
) -> Option<i32> {
    let js = JSValue::from_bits(callback.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let closure = js.as_pointer::<ClosureHeader>();
    if !crate::closure::is_closure_ptr(closure as usize)
        || unsafe { (*closure).code() } != js_wasi_import_call as *const u8
    {
        return None;
    }
    let syscall = crate::closure::js_closure_get_capture_f64(closure, 1) as usize;
    if perry_abi::wasi::PARAMS.get(syscall).copied() != Some(kinds) {
        return Some(perry_abi::wasi::IMPORT_TRAP);
    }
    let expected: &[u8] = if WASI_IMPORT_NAMES[syscall] == "proc_exit" {
        &[]
    } else {
        &[perry_abi::wasm_host::WASM_VAL_KIND_I32]
    };
    if results != expected {
        return Some(perry_abi::wasi::IMPORT_TRAP);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let import = scope.root_nanbox_f64(import_from_closure(
        crate::closure::plain_call_receiver(),
        closure,
    ));
    // A JS lifecycle/memory error must return through the interpreter before
    // it is surfaced, rather than longjmp across wasmi's live store borrow.
    Some(
        match crate::exception::catch_js_throw(|| call(import.get_nanbox_f64(), syscall, args)) {
            Ok(result) => {
                if result == perry_abi::wasi::IMPORT_EXIT {
                    crate::webassembly::record_import_exception(exit_marker(
                        import.get_nanbox_f64(),
                    ));
                }
                result
            }
            Err(error) => {
                crate::webassembly::record_import_exception(error);
                perry_abi::wasi::IMPORT_TRAP
            }
        },
    )
}
