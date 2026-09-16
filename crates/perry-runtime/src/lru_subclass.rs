//! #10293: `class X extends LRUCache` — give the name a usable base.
//!
//! Perry lowers `new LRUCache(options)` as a compile-time pattern on the
//! identifier (`lower_call/builtin.rs`), so `LRUCache` has no runtime value.
//! Reading it yields `undefined`, which is fine for a direct `new` and fatal
//! for anything using the name as a value: `class X extends LRUCache` throws
//! `TypeError: Class extends value is not a constructor`. `path-scurry` does
//! exactly that (`class ResolveCache extends LRUCache`), so `glob@9` and
//! everything under it fails to initialize.
//!
//! This is the shape `node:stream` and `EventEmitter` already use — HIR records
//! the native parent and `super(...)` lowers to a subclass-init that installs
//! the native methods onto `this` instead of constructing a JS base. The
//! node:stream entry's own comment names this exact error as what that path
//! exists to avoid.
//!
//! The cache lives behind the `js_lru_cache_*` C ABI, implemented by whichever
//! of `perry-ext-lru-cache` (the well-known binding) or perry-stdlib's
//! `bundled-lru-cache` the link selects. Exactly one is ever present and both
//! export the same symbols, so a plain `extern "C"` block binds to the live
//! one. This object file is pulled in only when codegen emits the
//! subclass-init, which happens only for a program that extends `LRUCache`.
//!
//! The small value helpers are local on purpose: `dgram`, where the crate's
//! equivalents live, is feature-gated, and the auto-optimized runtime is built
//! with feature sets that exclude it.

use crate::closure::ClosureHeader;
use crate::node_stream::dispatch::{
    cast0, cast1, cast2, install_methods_on_existing_object, StubFn,
};
use crate::object::ObjectHeader;
use crate::value::JSValue;

extern "C" {
    fn js_lru_cache_new(options: f64) -> i64;
    fn js_lru_cache_get(handle: i64, key: f64) -> f64;
    fn js_lru_cache_set(handle: i64, key: f64, value: f64) -> i64;
    fn js_lru_cache_has(handle: i64, key: f64) -> f64;
    fn js_lru_cache_delete(handle: i64, key: f64) -> f64;
    fn js_lru_cache_clear(handle: i64);
    fn js_lru_cache_peek(handle: i64, key: f64) -> f64;
}

/// Hidden slot on the subclass instance holding the native cache id. Stored as
/// a plain number, not a NaN-boxed pointer: handles are small monotonic ids,
/// exactly representable in an `f64`, and keeping it a number keeps the GC's
/// pointer scan away from a value that is not an address.
const HANDLE_KEY: &[u8] = b"__perry_lru_handle";

fn undefined_value() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn bool_value(v: bool) -> f64 {
    f64::from_bits(JSValue::bool(v).bits())
}

/// Normalise a predicate result to a JS boolean.
///
/// TWO `js_lru_cache_has`/`js_lru_cache_delete` symbols exist: perry-stdlib's
/// legacy pair answers a NUMBER (`1.0`/`0.0`) and perry-ext-lru-cache's answers
/// a NaN-boxed boolean. Which one these thunks bind to depends on the archive
/// set, while the DIRECT `cache.has(k)` lowering always reaches the ext one —
/// so without this a subclass diverged from its own base in the same program:
/// `base.has(k)` was `true` and `sub.has(k)` was `1`. Pass a real boolean
/// through untouched and convert a numeric answer.
fn as_bool_value(v: f64) -> f64 {
    let bits = v.to_bits();
    if bits == JSValue::bool(true).bits() || bits == JSValue::bool(false).bits() {
        return v;
    }
    bool_value(!v.is_nan() && v != 0.0)
}

fn hidden_key(bytes: &[u8]) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn object_ptr(value: f64) -> Option<*mut ObjectHeader> {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if !jsval.is_pointer() {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw < 0x10000 {
        return None;
    }
    unsafe {
        let header = crate::value::addr_class::try_read_tracked_gc_header(raw)?;
        if (*header.as_ptr()).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

/// The receiver `install_methods_on_existing_object` stores in capture 0.
fn this_value(closure: *const ClosureHeader) -> f64 {
    if !closure.is_null() {
        let bits = crate::closure::js_closure_get_capture_ptr(closure, 0) as u64;
        if bits != 0 {
            return f64::from_bits(bits);
        }
    }
    crate::object::js_implicit_this_get()
}

fn handle_of(this: f64) -> i64 {
    let Some(obj) = object_ptr(this) else {
        return 0;
    };
    let value = crate::object::js_object_get_field_by_name_f64(
        obj as *const ObjectHeader,
        hidden_key(HANDLE_KEY),
    );
    if value.is_finite() && value > 0.0 {
        value as i64
    } else {
        0
    }
}

extern "C" fn lru_m_get(closure: *const ClosureHeader, key: f64) -> f64 {
    let handle = handle_of(this_value(closure));
    if handle == 0 {
        return undefined_value();
    }
    unsafe { js_lru_cache_get(handle, key) }
}

extern "C" fn lru_m_set(closure: *const ClosureHeader, key: f64, value: f64) -> f64 {
    let this = this_value(closure);
    let handle = handle_of(this);
    if handle != 0 {
        unsafe { js_lru_cache_set(handle, key, value) };
    }
    // npm's `set` returns the cache, so `.set(a,1).set(b,2)` chains.
    this
}

extern "C" fn lru_m_has(closure: *const ClosureHeader, key: f64) -> f64 {
    let handle = handle_of(this_value(closure));
    if handle == 0 {
        return bool_value(false);
    }
    as_bool_value(unsafe { js_lru_cache_has(handle, key) })
}

extern "C" fn lru_m_delete(closure: *const ClosureHeader, key: f64) -> f64 {
    let handle = handle_of(this_value(closure));
    if handle == 0 {
        return bool_value(false);
    }
    as_bool_value(unsafe { js_lru_cache_delete(handle, key) })
}

extern "C" fn lru_m_peek(closure: *const ClosureHeader, key: f64) -> f64 {
    let handle = handle_of(this_value(closure));
    if handle == 0 {
        return undefined_value();
    }
    unsafe { js_lru_cache_peek(handle, key) }
}

extern "C" fn lru_m_clear(closure: *const ClosureHeader) -> f64 {
    let handle = handle_of(this_value(closure));
    if handle != 0 {
        unsafe { js_lru_cache_clear(handle) };
    }
    undefined_value()
}

/// Only the surface the binding implements is installed. `forEach`, `dispose`,
/// `fetch` and the iterator surface are deliberately absent so a use throws
/// `is not a function` at the call site — a better failure than the binding's
/// documented silent no-ops, which is why it is marked `partial` in
/// `well_known_bindings.toml`.
fn lru_methods() -> [(&'static str, StubFn); 6] {
    [
        ("get", cast1(lru_m_get)),
        ("set", cast2(lru_m_set)),
        ("has", cast1(lru_m_has)),
        ("delete", cast1(lru_m_delete)),
        ("peek", cast1(lru_m_peek)),
        ("clear", cast0(lru_m_clear)),
    ]
}

/// `super(options)` for a source-compiled `class X extends LRUCache`.
#[no_mangle]
pub extern "C" fn js_lru_cache_subclass_init(this: f64, opts: f64) -> f64 {
    if object_ptr(this).is_none() {
        return this;
    }
    let handle = unsafe { js_lru_cache_new(opts) };
    if handle == 0 {
        return this;
    }
    // Interning the key allocates and can move the receiver, so re-read the
    // object pointer after each allocating step rather than caching it.
    if let Some(obj) = object_ptr(this) {
        crate::object::js_object_set_field_by_name(obj, hidden_key(HANDLE_KEY), handle as f64);
    }
    let Some(obj) = object_ptr(this) else {
        return this;
    };
    let methods = lru_methods();
    install_methods_on_existing_object(obj, this, &methods, &[]);
    this
}

/// Link-time default for the `js_lru_cache_*` ABI on MSVC, and nowhere else.
///
/// The extern block above is satisfied by whichever cache provider the PROGRAM
/// links. A Rust binary that links perry-runtime without one still carries the
/// references, and two of them are built in CI on every Windows leg: the
/// `perry` compiler itself (`cargo build -p perry …`) and this crate's own
/// `--lib` test harness (`cargo test --lib -p perry-runtime`). Neither wants an
/// LRU cache; neither links a provider.
///
/// On every other target that is harmless, because the linker dead-strips
/// before it reports: `ld64 -dead_strip` / `ld --gc-sections` drop the thunks
/// above out of a binary that never calls them, and the references go with
/// them. Verified on macOS — the linked `target/perry-dev/perry` contains no
/// `js_lru_cache_subclass_init` symbol at all, and the build succeeds while the
/// rlib it links still shows all seven as `U`. `link.exe` resolves symbols
/// BEFORE `/OPT:REF`, so the same inputs are 7 × LNK2019 there.
///
/// A Cargo feature cannot express "this link has no provider". The Windows job
/// builds `-p perry -p perry-runtime-static -p perry-stdlib-static` in ONE
/// invocation, so perry-stdlib's `perry-runtime/stdlib` feature is unified onto
/// the copy of perry-runtime that the `perry` binary links — even though
/// perry-stdlib is not in that binary's link. Anything gated on `stdlib`
/// (`crate::stdlib_stubs`, an `external-*-symbols` flag) is therefore compiled
/// out in exactly the configuration that fails.
///
/// `/ALTERNATENAME` is MSVC's spelling of a weak default: link.exe substitutes
/// the alternate only for a symbol still undefined after every input has been
/// read. A program that does link `perry_stdlib.lib` or the ext archive binds
/// the real implementation and never reaches these — so this cannot shadow a
/// provider the way an unconditional definition would. They live in this module
/// so that they share a codegen unit with the thunks whose references they
/// answer.
///
/// `js_lru_cache_new` answering 0 is already the "no cache" path: the
/// subclass-init returns `this` without installing any method, so a `.get()` on
/// it throws `is not a function` at the call site — the same failure this
/// module deliberately chooses for `forEach`/`dispose`/`fetch`.
#[cfg(all(windows, target_env = "msvc"))]
mod msvc_absent_provider {
    use crate::stub_diag::perry_stub_warn;

    const REASON: &str =
        "no lru-cache provider (perry-ext-lru-cache / perry-stdlib bundled-lru-cache) \
         is linked into this binary";

    /// Emit one `/ALTERNATENAME:<symbol>=<fallback>` linker directive.
    macro_rules! alternatename {
        ($stat:ident, $bytes:literal) => {
            #[used]
            #[link_section = ".drectve"]
            static $stat: [u8; $bytes.len()] = *$bytes;
        };
    }

    alternatename!(
        D_NEW,
        b" /ALTERNATENAME:js_lru_cache_new=perry_lru_cache_absent_new"
    );
    alternatename!(
        D_GET,
        b" /ALTERNATENAME:js_lru_cache_get=perry_lru_cache_absent_get"
    );
    alternatename!(
        D_SET,
        b" /ALTERNATENAME:js_lru_cache_set=perry_lru_cache_absent_set"
    );
    alternatename!(
        D_HAS,
        b" /ALTERNATENAME:js_lru_cache_has=perry_lru_cache_absent_has"
    );
    alternatename!(
        D_DELETE,
        b" /ALTERNATENAME:js_lru_cache_delete=perry_lru_cache_absent_delete"
    );
    alternatename!(
        D_CLEAR,
        b" /ALTERNATENAME:js_lru_cache_clear=perry_lru_cache_absent_clear"
    );
    alternatename!(
        D_PEEK,
        b" /ALTERNATENAME:js_lru_cache_peek=perry_lru_cache_absent_peek"
    );

    #[no_mangle]
    pub extern "C" fn perry_lru_cache_absent_new(_options: f64) -> i64 {
        perry_stub_warn("js_lru_cache_new", REASON, None);
        0
    }

    #[no_mangle]
    pub extern "C" fn perry_lru_cache_absent_get(_handle: i64, _key: f64) -> f64 {
        perry_stub_warn("js_lru_cache_get", REASON, None);
        super::undefined_value()
    }

    #[no_mangle]
    pub extern "C" fn perry_lru_cache_absent_set(handle: i64, _key: f64, _value: f64) -> i64 {
        perry_stub_warn("js_lru_cache_set", REASON, None);
        handle
    }

    #[no_mangle]
    pub extern "C" fn perry_lru_cache_absent_has(_handle: i64, _key: f64) -> f64 {
        perry_stub_warn("js_lru_cache_has", REASON, None);
        super::bool_value(false)
    }

    #[no_mangle]
    pub extern "C" fn perry_lru_cache_absent_delete(_handle: i64, _key: f64) -> f64 {
        perry_stub_warn("js_lru_cache_delete", REASON, None);
        super::bool_value(false)
    }

    #[no_mangle]
    pub extern "C" fn perry_lru_cache_absent_clear(_handle: i64) {
        perry_stub_warn("js_lru_cache_clear", REASON, None);
    }

    #[no_mangle]
    pub extern "C" fn perry_lru_cache_absent_peek(_handle: i64, _key: f64) -> f64 {
        perry_stub_warn("js_lru_cache_peek", REASON, None);
        super::undefined_value()
    }
}
