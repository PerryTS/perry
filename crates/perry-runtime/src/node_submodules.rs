//! Issue #841 — wire up named exports + namespace imports for five
//! Node.js submodules that Perry's manifest had registered but whose
//! FFI export tables defaulted to a `TAG_TRUE` sentinel cell:
//!
//!   - `node:timers/promises` (setTimeout / setImmediate / setInterval / scheduler.*)
//!   - `node:readline/promises` (createInterface, Interface, Readline)
//!   - `node:stream/promises` (pipeline, finished)
//!   - `node:stream/consumers` (text, json, buffer, arrayBuffer, bytes, blob)
//!   - `node:sys` (deprecated alias for node:util — re-exports format, inspect, etc.)
//!
//! Pre-fix `import { setTimeout } from "node:timers/promises"; typeof setTimeout`
//! reported `"boolean"` (the value was literally `true`) and `import * as ns
//! from "node:..."` errored at compile time with the "switch to named imports"
//! diagnostic. This module ships per-export function singletons whose `typeof`
//! is `"function"`, plus per-submodule namespace stubs whose properties point
//! at the same singletons.
//!
//! The thunks are deliberately minimal — they throw `Error("<api> is not yet
//! implemented in Perry")` when invoked. Full functional implementations of
//! these APIs are tracked separately under the #793 Node compatibility
//! roadmap. The fix here is strictly about restoring the import surface so
//! consuming code can at least introspect the bindings (typeof checks,
//! `=== util.format` comparisons, dynamic-shape introspection) without
//! tripping over `true`-as-a-function downstream errors.

use std::cell::RefCell;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::closure::{js_closure_alloc, ClosureHeader};
use crate::object::{js_object_alloc, ObjectHeader};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

/// One entry per named export of one submodule.
struct ExportSpec {
    name: &'static str,
    thunk: extern "C" fn(*const ClosureHeader, f64) -> f64,
}

/// One entry per submodule. `exports` lists every named export the
/// codegen / parity tests reach for; the codegen's lookup is keyed by
/// `(submodule_key, export_name)` and falls back to `TAG_TRUE` if no
/// matching entry is found (preserving the pre-#841 behavior for any
/// future export Perry doesn't yet know about).
struct SubmoduleSpec {
    /// Stable key — matches the prefix used in the generated FFI symbol
    /// names (`js_node_submod_<key>_export_<name>`).
    key: &'static str,
    exports: &'static [ExportSpec],
}

// ----- thunks -----
//
// One thunk per (submodule, export). All thunks share the same shape:
// they raise an explicit `Error` describing what's missing. Closure
// dispatch invokes them via `js_closure_call0` / `js_closure_call1`
// regardless of declared arity, so a single `(_closure, _arg) -> f64`
// signature is sufficient — Perry's closure ABI tolerates an arg shape
// mismatch on the receiving side (the value is just ignored).

macro_rules! thunk {
    ($name:ident, $msg:expr) => {
        extern "C" fn $name(_closure: *const ClosureHeader, _arg: f64) -> f64 {
            let msg: &'static str = $msg;
            let bytes = msg.as_bytes();
            let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let err = crate::error::js_error_new_with_message(header);
            let bits = JSValue::pointer(err as *const u8).bits();
            crate::exception::js_throw(f64::from_bits(bits))
        }
    };
}

thunk!(
    thunk_timers_setTimeout,
    "node:timers/promises.setTimeout is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_timers_setImmediate,
    "node:timers/promises.setImmediate is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_timers_setInterval,
    "node:timers/promises.setInterval is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_timers_scheduler,
    "node:timers/promises.scheduler is not yet implemented in Perry (tracked by issue #793)."
);

thunk!(thunk_readline_createInterface, "node:readline/promises.createInterface is not yet implemented in Perry (tracked by issue #793).");
thunk!(
    thunk_readline_Interface,
    "node:readline/promises.Interface is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_readline_Readline,
    "node:readline/promises.Readline is not yet implemented in Perry (tracked by issue #793)."
);

thunk!(
    thunk_streamP_pipeline,
    "node:stream/promises.pipeline is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_streamP_finished,
    "node:stream/promises.finished is not yet implemented in Perry (tracked by issue #793)."
);

thunk!(
    thunk_consumers_text,
    "node:stream/consumers.text is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_json,
    "node:stream/consumers.json is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_buffer,
    "node:stream/consumers.buffer is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_arrayBuffer,
    "node:stream/consumers.arrayBuffer is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_bytes,
    "node:stream/consumers.bytes is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_blob,
    "node:stream/consumers.blob is not yet implemented in Perry (tracked by issue #793)."
);

// node:sys is a deprecated alias for node:util — point each export at
// the same thunks until util's named-export surface is wired up. The
// parity test compares `sys.format === util.format` for identity; for
// now both report `typeof === "function"` (passing the typeof gate) but
// the strict-equality check still diverges. That divergence is
// pre-existing (node:util's named exports lower to NativeModuleRef =>
// `typeof === "object"` today) — it's the parent-module half of #793.
thunk!(thunk_sys_format, "node:sys.format is not yet implemented in Perry (use node:util.format; node:sys is deprecated).");
thunk!(thunk_sys_inspect, "node:sys.inspect is not yet implemented in Perry (use node:util.inspect; node:sys is deprecated).");
thunk!(thunk_sys_debuglog, "node:sys.debuglog is not yet implemented in Perry (use node:util.debuglog; node:sys is deprecated).");
thunk!(thunk_sys_deprecate, "node:sys.deprecate is not yet implemented in Perry (use node:util.deprecate; node:sys is deprecated).");
thunk!(thunk_sys_promisify, "node:sys.promisify is not yet implemented in Perry (use node:util.promisify; node:sys is deprecated).");
thunk!(thunk_sys_callbackify, "node:sys.callbackify is not yet implemented in Perry (use node:util.callbackify; node:sys is deprecated).");
thunk!(thunk_sys_isArray, "node:sys.isArray is not yet implemented in Perry (use node:util.isArray; node:sys is deprecated).");

// ----- submodule table -----

const SUBMODULES: &[SubmoduleSpec] = &[
    SubmoduleSpec {
        key: "timers_promises",
        exports: &[
            ExportSpec {
                name: "setTimeout",
                thunk: thunk_timers_setTimeout,
            },
            ExportSpec {
                name: "setImmediate",
                thunk: thunk_timers_setImmediate,
            },
            ExportSpec {
                name: "setInterval",
                thunk: thunk_timers_setInterval,
            },
            ExportSpec {
                name: "scheduler",
                thunk: thunk_timers_scheduler,
            },
        ],
    },
    SubmoduleSpec {
        key: "readline_promises",
        exports: &[
            ExportSpec {
                name: "createInterface",
                thunk: thunk_readline_createInterface,
            },
            ExportSpec {
                name: "Interface",
                thunk: thunk_readline_Interface,
            },
            ExportSpec {
                name: "Readline",
                thunk: thunk_readline_Readline,
            },
        ],
    },
    SubmoduleSpec {
        key: "stream_promises",
        exports: &[
            ExportSpec {
                name: "pipeline",
                thunk: thunk_streamP_pipeline,
            },
            ExportSpec {
                name: "finished",
                thunk: thunk_streamP_finished,
            },
        ],
    },
    SubmoduleSpec {
        key: "stream_consumers",
        exports: &[
            ExportSpec {
                name: "text",
                thunk: thunk_consumers_text,
            },
            ExportSpec {
                name: "json",
                thunk: thunk_consumers_json,
            },
            ExportSpec {
                name: "buffer",
                thunk: thunk_consumers_buffer,
            },
            ExportSpec {
                name: "arrayBuffer",
                thunk: thunk_consumers_arrayBuffer,
            },
            ExportSpec {
                name: "bytes",
                thunk: thunk_consumers_bytes,
            },
            ExportSpec {
                name: "blob",
                thunk: thunk_consumers_blob,
            },
        ],
    },
    SubmoduleSpec {
        key: "sys",
        exports: &[
            ExportSpec {
                name: "format",
                thunk: thunk_sys_format,
            },
            ExportSpec {
                name: "inspect",
                thunk: thunk_sys_inspect,
            },
            ExportSpec {
                name: "debuglog",
                thunk: thunk_sys_debuglog,
            },
            ExportSpec {
                name: "deprecate",
                thunk: thunk_sys_deprecate,
            },
            ExportSpec {
                name: "promisify",
                thunk: thunk_sys_promisify,
            },
            ExportSpec {
                name: "callbackify",
                thunk: thunk_sys_callbackify,
            },
            ExportSpec {
                name: "isArray",
                thunk: thunk_sys_isArray,
            },
        ],
    },
];

fn find_submodule(key: &str) -> Option<&'static SubmoduleSpec> {
    SUBMODULES.iter().find(|s| s.key == key)
}

fn find_export(submod: &SubmoduleSpec, name: &str) -> Option<&'static ExportSpec> {
    submod.exports.iter().find(|e| e.name == name)
}

// ----- singleton storage -----
//
// One AtomicI64 slot per thunk so concurrent first-use callers don't
// leak a closure. Stored in a thread_local Vec for simplicity — these
// singletons are allocated on first reach and live until process exit
// (they're root-marked by `scan_node_submodule_singleton_roots` below).

thread_local! {
    /// Map from (submod_key_ptr, export_name_ptr) — both `&'static str`,
    /// so pointer-equality is sufficient — to the cached singleton
    /// ClosureHeader pointer for that export's thunk.
    static EXPORT_SINGLETONS: RefCell<std::collections::HashMap<(usize, usize), *mut ClosureHeader>> =
        RefCell::new(std::collections::HashMap::new());

    /// Map from submod_key_ptr to the cached namespace ObjectHeader
    /// pointer — populated once per submodule on first namespace use.
    static NAMESPACE_SINGLETONS: RefCell<std::collections::HashMap<usize, *mut ObjectHeader>> =
        RefCell::new(std::collections::HashMap::new());
}

// We also need a process-wide "any singleton allocated?" flag so the
// GC scanner can early-out without taking the thread_local borrow on
// every cycle. Using `AtomicI64` instead of `AtomicBool` so the scanner
// can also use it as a release fence against the thread_local writes.
static ANY_SINGLETON_ALLOCATED: AtomicI64 = AtomicI64::new(0);

fn ensure_export_singleton(
    submod: &'static SubmoduleSpec,
    export: &'static ExportSpec,
) -> *mut ClosureHeader {
    let key = (submod.key.as_ptr() as usize, export.name.as_ptr() as usize);
    if let Some(cached) = EXPORT_SINGLETONS.with(|m| m.borrow().get(&key).copied()) {
        return cached;
    }
    let allocated = js_closure_alloc(export.thunk as *const u8, 0);
    EXPORT_SINGLETONS.with(|m| {
        m.borrow_mut().insert(key, allocated);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    allocated
}

fn ensure_namespace_singleton(submod: &'static SubmoduleSpec) -> *mut ObjectHeader {
    let key = submod.key.as_ptr() as usize;
    if let Some(cached) = NAMESPACE_SINGLETONS.with(|m| m.borrow().get(&key).copied()) {
        return cached;
    }
    // Allocate a fresh object with one inline slot per known export;
    // the dynamic-property path in `js_object_set_field_by_name` will
    // grow it if needed.
    let field_count = submod.exports.len() as u32;
    let obj = js_object_alloc(0, field_count);
    // Populate fields. Each export's value is the singleton closure
    // pointer NaN-boxed as POINTER. We route through
    // `js_object_set_field_by_name` so the keys array gets built up
    // identically to what user code's literal object init would
    // produce — that's what `js_object_keys` / spread / Reflect.ownKeys
    // walks at runtime.
    for spec in submod.exports {
        let closure_ptr = ensure_export_singleton(submod, spec);
        let value_bits = JSValue::pointer(closure_ptr as *const u8).bits();
        let value_f64 = f64::from_bits(value_bits);
        unsafe {
            let name_bytes = spec.name.as_bytes();
            let name_header = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            crate::object::js_object_set_field_by_name(obj, name_header, value_f64);
        }
    }
    NAMESPACE_SINGLETONS.with(|m| {
        m.borrow_mut().insert(key, obj);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    obj
}

/// GC root scanner: pin every (export-singleton, namespace-singleton)
/// allocated by this module against the next sweep. Wired up from
/// `gc::gc_init`.
pub fn scan_node_submodule_singleton_roots(mark: &mut dyn FnMut(f64)) {
    if ANY_SINGLETON_ALLOCATED.load(Ordering::Acquire) == 0 {
        return;
    }
    EXPORT_SINGLETONS.with(|m| {
        for &closure_ptr in m.borrow().values() {
            let v = JSValue::pointer(closure_ptr as *const u8);
            mark(f64::from_bits(v.bits()));
        }
    });
    NAMESPACE_SINGLETONS.with(|m| {
        for &obj_ptr in m.borrow().values() {
            let v = JSValue::pointer(obj_ptr as *const u8);
            mark(f64::from_bits(v.bits()));
        }
    });
}

// ----- FFI entry points -----
//
// `submod_key_ptr` / `name_ptr` are `*const u8` pointers + lengths
// rather than NUL-terminated strings so codegen can hand off the raw
// bytes from emitted IR (already produced as `private constant
// [N x i8]` arrays via `emit_string_literal`).

/// Returns a NaN-boxed function singleton for the given
/// `(submodule, export)` pair. Falls back to NaN-boxed `TAG_TRUE`
/// (preserving the pre-#841 sentinel) if no matching entry is found —
/// this keeps any not-yet-listed export's behavior unchanged, so
/// later additions to `SUBMODULES` are strictly additive.
///
/// # Safety
///
/// The `submod_key_ptr` / `name_ptr` arguments must point to valid UTF-8
/// byte sequences of the indicated length, and remain alive for the
/// duration of this call.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_export_as_function(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
    name_ptr: *const u8,
    name_len: u32,
) -> f64 {
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let export = match find_export(submod, name) {
        Some(e) => e,
        None => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let closure_ptr = ensure_export_singleton(submod, export);
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

/// Returns a NaN-boxed namespace stub object for the given submodule.
/// Each known named export of that submodule is exposed as an own
/// property on the object whose value is the function singleton
/// produced by `js_node_submodule_export_as_function`. Falls back to
/// `js_unresolved_namespace_stub` (the empty-object stub Perry already
/// hands out for unknown namespace imports) if `submod_key` doesn't
/// match a known submodule.
///
/// # Safety
///
/// Same constraints as `js_node_submodule_export_as_function`.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_namespace(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
) -> f64 {
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return crate::object::js_unresolved_namespace_stub(),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return crate::object::js_unresolved_namespace_stub(),
    };
    let obj = ensure_namespace_singleton(submod);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_submodules_have_at_least_one_export() {
        for s in SUBMODULES {
            assert!(
                !s.exports.is_empty(),
                "submodule {} has zero exports",
                s.key
            );
        }
    }

    #[test]
    fn find_submodule_for_known_keys() {
        for key in [
            "timers_promises",
            "readline_promises",
            "stream_promises",
            "stream_consumers",
            "sys",
        ] {
            assert!(
                find_submodule(key).is_some(),
                "submodule {} missing from SUBMODULES table",
                key
            );
        }
    }

    #[test]
    fn find_submodule_for_unknown_key_returns_none() {
        assert!(find_submodule("not_a_real_submodule").is_none());
    }
}
