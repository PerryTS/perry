//! Per-module native-module method-dispatch registry (devirtualization).
//! GENERATED scaffolding — see NM_DEVIRT_PLAN.md. Each `js_nm_install_<m>()` is
//! the SOLE static reference to its `nm_dispatch_<m>` bucket fn; codegen emits a
//! call per statically-imported native module so the linker dead-strips the rest.
//! NOTHING here names all buckets together (that would re-pin everything).
use super::native_module_dispatch::{
    nm_dispatch_assert, nm_dispatch_async_hooks, nm_dispatch_bigint, nm_dispatch_buffer,
    nm_dispatch_bun, nm_dispatch_bun_ffi, nm_dispatch_child_process, nm_dispatch_cluster,
    nm_dispatch_console, nm_dispatch_crypto, nm_dispatch_dgram, nm_dispatch_dns,
    nm_dispatch_domain, nm_dispatch_events, nm_dispatch_fs, nm_dispatch_http,
    nm_dispatch_inspector, nm_dispatch_module, nm_dispatch_net, nm_dispatch_node_pty,
    nm_dispatch_os, nm_dispatch_path, nm_dispatch_perf, nm_dispatch_process, nm_dispatch_punycode,
    nm_dispatch_querystring, nm_dispatch_readline, nm_dispatch_repl, nm_dispatch_sea,
    nm_dispatch_sqlite, nm_dispatch_stream, nm_dispatch_timers, nm_dispatch_tls, nm_dispatch_tty,
    nm_dispatch_url, nm_dispatch_util, nm_dispatch_v8, nm_dispatch_vm, nm_dispatch_wasi,
    nm_dispatch_zlib, NmCtx,
};
use std::sync::atomic::{AtomicPtr, Ordering};

type NmDispatchFn = unsafe fn(&NmCtx, &str, &str) -> f64;

#[derive(Copy, Clone)]
#[repr(usize)]
enum NmBucket {
    Assert,
    AsyncHooks,
    Bigint,
    Buffer,
    Bun,
    BunFfi,
    ChildProcess,
    Cluster,
    Console,
    Crypto,
    Dgram,
    Dns,
    Domain,
    Events,
    Fs,
    Http,
    Inspector,
    Module,
    Net,
    NodePty,
    Os,
    Path,
    Perf,
    Process,
    Punycode,
    Querystring,
    Readline,
    Repl,
    Sea,
    Sqlite,
    Stream,
    Timers,
    Tls,
    Tty,
    Url,
    Util,
    V8,
    Vm,
    Wasi,
    Zlib,
}
const NM_BUCKET_COUNT: usize = 40;
// #6580 merge: inserting BunFfi shifted every later NmBucket up one; keep this array
// big enough to index every variant (Zlib is the last). Guard against silent overflow.
const _: () = assert!((NmBucket::Zlib as usize) < NM_BUCKET_COUNT);

static NM_DISPATCH_REGISTRY: [AtomicPtr<()>; NM_BUCKET_COUNT] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; NM_BUCKET_COUNT];

/// Map a (normalized) module tag to its bucket. Pure string match — references
/// no bucket fn, so it pins nothing.
fn nm_module_index(name: &str) -> Option<NmBucket> {
    match name {
        "assert" | "assert/strict" => Some(NmBucket::Assert),
        "async_hooks" => Some(NmBucket::AsyncHooks),
        "bigint" => Some(NmBucket::Bigint),
        "buffer" | "buffer.Buffer" => Some(NmBucket::Buffer),
        "bun" => Some(NmBucket::Bun),
        // #6562: the `bun:` prefix is part of the name (not stripped like
        // `node:`).
        "bun:ffi" => Some(NmBucket::BunFfi),
        "child_process" => Some(NmBucket::ChildProcess),
        "cluster" => Some(NmBucket::Cluster),
        "console" => Some(NmBucket::Console),
        "crypto" | "crypto.Certificate" | "crypto.KeyObject" | "crypto.subtle"
        | "crypto.webcrypto" => Some(NmBucket::Crypto),
        "dgram" => Some(NmBucket::Dgram),
        "dns" | "dns/promises" => Some(NmBucket::Dns),
        "domain" => Some(NmBucket::Domain),
        "events" => Some(NmBucket::Events),
        "fs" => Some(NmBucket::Fs),
        "http" | "http2" | "https" => Some(NmBucket::Http),
        "inspector" | "inspector.Network" | "inspector/promises" => Some(NmBucket::Inspector),
        "module" => Some(NmBucket::Module),
        "net" => Some(NmBucket::Net),
        // #6563: node-pty + the API-identical @lydell fork, one bucket.
        "node-pty" | "@lydell/node-pty" => Some(NmBucket::NodePty),
        "os" => Some(NmBucket::Os),
        "path" | "path.posix" | "path.win32" => Some(NmBucket::Path),
        "perf_histogram" | "perf_hooks" | "perf_observer" | "perf_observer_list" => {
            Some(NmBucket::Perf)
        }
        "process" => Some(NmBucket::Process),
        "punycode" | "punycode.ucs2" | "punycode.default" => Some(NmBucket::Punycode),
        "querystring" => Some(NmBucket::Querystring),
        "readline" => Some(NmBucket::Readline),
        "repl" => Some(NmBucket::Repl),
        "sea" => Some(NmBucket::Sea),
        "sqlite" => Some(NmBucket::Sqlite),
        "stream" => Some(NmBucket::Stream),
        "timers" => Some(NmBucket::Timers),
        "tls" => Some(NmBucket::Tls),
        "tty" => Some(NmBucket::Tty),
        "url" => Some(NmBucket::Url),
        "util" | "util.types" | "util/types" => Some(NmBucket::Util),
        "v8"
        | "v8.Deserializer"
        | "v8.GCProfiler"
        | "v8.Serializer"
        | "v8.promiseHooks"
        | "v8.startupSnapshot"
        | "v8.DefaultSerializer"
        | "v8.DefaultDeserializer" => Some(NmBucket::V8),
        "vm" => Some(NmBucket::Vm),
        "wasi" => Some(NmBucket::Wasi),
        "zlib" => Some(NmBucket::Zlib),
        _ => None,
    }
}

/// Look up the installed per-module dispatch fn for `name`. `None` if unknown or
/// its `js_nm_install_<m>()` was never emitted (module not statically imported).
pub(crate) fn nm_dispatch_lookup(name: &str) -> Option<NmDispatchFn> {
    let b = nm_module_index(name)?;
    let p = NM_DISPATCH_REGISTRY[b as usize].load(Ordering::Relaxed);
    if !p.is_null() {
        return Some(unsafe { std::mem::transmute::<*mut (), NmDispatchFn>(p) });
    }
    // Unit tests call dispatch directly, without the codegen-emitted
    // `js_nm_install_<module>()` that precedes use in real programs. Lazily
    // populate so tests exercise the real registry path. (Not in production.)
    #[cfg(test)]
    {
        js_nm_install_all();
        let p = NM_DISPATCH_REGISTRY[b as usize].load(Ordering::Relaxed);
        if !p.is_null() {
            return Some(unsafe { std::mem::transmute::<*mut (), NmDispatchFn>(p) });
        }
    }
    None
}

#[no_mangle]
pub extern "C" fn js_nm_install_assert() {
    NM_DISPATCH_REGISTRY[NmBucket::Assert as usize].store(
        nm_dispatch_assert as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
/// Seed `globalThis.AsyncLocalStorage` with the `async_hooks` constructor
/// BEFORE any module init runs. Next.js's `node-environment-baseline.js` does
/// exactly this assignment, but only when its own init runs — and several
/// Next modules (including the bundled app-page runtime) snapshot
/// `globalThis.AsyncLocalStorage` at *their* module scope. Perry's eager init
/// order can run those snapshots first, leaving them with `undefined` and a
/// FakeAsyncLocalStorage that throws `Invariant: AsyncLocalStorage accessed in
/// runtime where it is not available` (Next E504) on the first request.
/// Emitted by codegen at the top of the entry `main`, so the value exists
/// before ANY module-scope code observes it. (Divergence note: Node itself
/// does not expose AsyncLocalStorage on globalThis; programs feature-detecting
/// its absence will see it present under Perry.)
#[no_mangle]
pub extern "C" fn js_globalthis_seed_async_local_storage() {
    let global = crate::object::js_get_global_this();
    let ctor = crate::object::native_module::bound_native_callable_export_value(
        "async_hooks",
        "AsyncLocalStorage",
    );
    let key = crate::string::js_string_from_bytes(b"AsyncLocalStorage".as_ptr(), 17);
    crate::object::js_object_set_field_by_name(
        crate::value::js_nanbox_get_pointer(global) as *mut crate::object::ObjectHeader,
        key,
        ctor,
    );
}

#[no_mangle]
pub extern "C" fn js_nm_install_async_hooks() {
    NM_DISPATCH_REGISTRY[NmBucket::AsyncHooks as usize].store(
        nm_dispatch_async_hooks as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_bigint() {
    NM_DISPATCH_REGISTRY[NmBucket::Bigint as usize].store(
        nm_dispatch_bigint as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_buffer() {
    NM_DISPATCH_REGISTRY[NmBucket::Buffer as usize].store(
        nm_dispatch_buffer as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
/// bun:ffi (#6562).
#[no_mangle]
pub extern "C" fn js_nm_install_bun_ffi() {
    NM_DISPATCH_REGISTRY[NmBucket::BunFfi as usize].store(
        nm_dispatch_bun_ffi as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_bun() {
    NM_DISPATCH_REGISTRY[NmBucket::Bun as usize].store(
        nm_dispatch_bun as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_child_process() {
    NM_DISPATCH_REGISTRY[NmBucket::ChildProcess as usize].store(
        nm_dispatch_child_process as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_cluster() {
    NM_DISPATCH_REGISTRY[NmBucket::Cluster as usize].store(
        nm_dispatch_cluster as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_console() {
    NM_DISPATCH_REGISTRY[NmBucket::Console as usize].store(
        nm_dispatch_console as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_crypto() {
    NM_DISPATCH_REGISTRY[NmBucket::Crypto as usize].store(
        nm_dispatch_crypto as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_dgram() {
    NM_DISPATCH_REGISTRY[NmBucket::Dgram as usize].store(
        nm_dispatch_dgram as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_dns() {
    NM_DISPATCH_REGISTRY[NmBucket::Dns as usize].store(
        nm_dispatch_dns as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_domain() {
    NM_DISPATCH_REGISTRY[NmBucket::Domain as usize].store(
        nm_dispatch_domain as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_events() {
    NM_DISPATCH_REGISTRY[NmBucket::Events as usize].store(
        nm_dispatch_events as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_fs() {
    NM_DISPATCH_REGISTRY[NmBucket::Fs as usize]
        .store(nm_dispatch_fs as NmDispatchFn as *mut (), Ordering::Relaxed);
    nm_register_ctor(NmBucket::Fs, nm_ctor_fs);
}
#[no_mangle]
pub extern "C" fn js_nm_install_http() {
    NM_DISPATCH_REGISTRY[NmBucket::Http as usize].store(
        nm_dispatch_http as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_inspector() {
    NM_DISPATCH_REGISTRY[NmBucket::Inspector as usize].store(
        nm_dispatch_inspector as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_module() {
    NM_DISPATCH_REGISTRY[NmBucket::Module as usize].store(
        nm_dispatch_module as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_net() {
    NM_DISPATCH_REGISTRY[NmBucket::Net as usize].store(
        nm_dispatch_net as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_node_pty() {
    NM_DISPATCH_REGISTRY[NmBucket::NodePty as usize].store(
        nm_dispatch_node_pty as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_os() {
    NM_DISPATCH_REGISTRY[NmBucket::Os as usize]
        .store(nm_dispatch_os as NmDispatchFn as *mut (), Ordering::Relaxed);
}
#[no_mangle]
pub extern "C" fn js_nm_install_path() {
    NM_DISPATCH_REGISTRY[NmBucket::Path as usize].store(
        nm_dispatch_path as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_perf() {
    NM_DISPATCH_REGISTRY[NmBucket::Perf as usize].store(
        nm_dispatch_perf as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_process() {
    NM_DISPATCH_REGISTRY[NmBucket::Process as usize].store(
        nm_dispatch_process as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_punycode() {
    NM_DISPATCH_REGISTRY[NmBucket::Punycode as usize].store(
        nm_dispatch_punycode as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_querystring() {
    NM_DISPATCH_REGISTRY[NmBucket::Querystring as usize].store(
        nm_dispatch_querystring as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_readline() {
    NM_DISPATCH_REGISTRY[NmBucket::Readline as usize].store(
        nm_dispatch_readline as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
    nm_register_ctor(NmBucket::Readline, nm_ctor_readline);
}
#[no_mangle]
pub extern "C" fn js_nm_install_repl() {
    NM_DISPATCH_REGISTRY[NmBucket::Repl as usize].store(
        nm_dispatch_repl as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
    nm_register_ctor(NmBucket::Repl, nm_ctor_repl);
}
#[no_mangle]
pub extern "C" fn js_nm_install_sea() {
    NM_DISPATCH_REGISTRY[NmBucket::Sea as usize].store(
        nm_dispatch_sea as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_sqlite() {
    NM_DISPATCH_REGISTRY[NmBucket::Sqlite as usize].store(
        nm_dispatch_sqlite as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_stream() {
    NM_DISPATCH_REGISTRY[NmBucket::Stream as usize].store(
        nm_dispatch_stream as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
    nm_register_ctor(NmBucket::Stream, nm_ctor_stream);
}
#[no_mangle]
pub extern "C" fn js_nm_install_timers() {
    NM_DISPATCH_REGISTRY[NmBucket::Timers as usize].store(
        nm_dispatch_timers as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_tls() {
    NM_DISPATCH_REGISTRY[NmBucket::Tls as usize].store(
        nm_dispatch_tls as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
    nm_register_ctor(NmBucket::Tls, nm_ctor_tls);
}
#[no_mangle]
pub extern "C" fn js_nm_install_tty() {
    NM_DISPATCH_REGISTRY[NmBucket::Tty as usize].store(
        nm_dispatch_tty as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
    nm_register_ctor(NmBucket::Tty, nm_ctor_tty);
}
#[no_mangle]
pub extern "C" fn js_nm_install_url() {
    NM_DISPATCH_REGISTRY[NmBucket::Url as usize].store(
        nm_dispatch_url as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_util() {
    NM_DISPATCH_REGISTRY[NmBucket::Util as usize].store(
        nm_dispatch_util as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_nm_install_v8() {
    NM_DISPATCH_REGISTRY[NmBucket::V8 as usize]
        .store(nm_dispatch_v8 as NmDispatchFn as *mut (), Ordering::Relaxed);
}
#[no_mangle]
pub extern "C" fn js_nm_install_vm() {
    NM_DISPATCH_REGISTRY[NmBucket::Vm as usize]
        .store(nm_dispatch_vm as NmDispatchFn as *mut (), Ordering::Relaxed);
    nm_register_ctor(NmBucket::Vm, nm_ctor_vm);
}
#[no_mangle]
pub extern "C" fn js_nm_install_wasi() {
    NM_DISPATCH_REGISTRY[NmBucket::Wasi as usize].store(
        nm_dispatch_wasi as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
    nm_register_ctor(NmBucket::Wasi, nm_ctor_wasi);
}
#[no_mangle]
pub extern "C" fn js_nm_install_zlib() {
    NM_DISPATCH_REGISTRY[NmBucket::Zlib as usize].store(
        nm_dispatch_zlib as NmDispatchFn as *mut (),
        Ordering::Relaxed,
    );
}

/// Dynamic-require fallback: register every bucket. Emitted by codegen only when
/// a native module is imported under an unanalyzable (dynamic) name.
#[no_mangle]
pub extern "C" fn js_nm_install_all() {
    js_nm_install_assert();
    js_nm_install_async_hooks();
    js_nm_install_bigint();
    js_nm_install_buffer();
    js_nm_install_bun();
    js_nm_install_bun_ffi();
    js_nm_install_child_process();
    js_nm_install_cluster();
    js_nm_install_console();
    js_nm_install_crypto();
    js_nm_install_dgram();
    js_nm_install_dns();
    js_nm_install_domain();
    js_nm_install_events();
    js_nm_install_fs();
    js_nm_install_http();
    js_nm_install_inspector();
    js_nm_install_module();
    js_nm_install_net();
    js_nm_install_node_pty();
    js_nm_install_os();
    js_nm_install_path();
    js_nm_install_perf();
    js_nm_install_process();
    js_nm_install_punycode();
    js_nm_install_querystring();
    js_nm_install_readline();
    js_nm_install_repl();
    js_nm_install_sea();
    js_nm_install_sqlite();
    js_nm_install_stream();
    js_nm_install_timers();
    js_nm_install_tls();
    js_nm_install_tty();
    js_nm_install_url();
    js_nm_install_util();
    js_nm_install_v8();
    js_nm_install_vm();
    js_nm_install_wasi();
    js_nm_install_zlib();
}

// ── Dynamic-require install-all hook ───────────────────────────────────────
// `js_nm_install_all` names every bucket, so it must NOT be statically reachable
// from any always-linked function (the runtime builtin resolver is linked into
// every program via `process.getBuiltinModule`). Instead it is reached only
// through this indirect pointer, set by `js_nm_enable_install_all()` which
// codegen emits ONLY when the program uses dynamic `require`/`getBuiltinModule`.
// Programs without dynamic builtin require never reference the setter → both it
// and `js_nm_install_all` dead-strip, preserving precise per-module stripping.
static NM_INSTALL_ALL_HOOK: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Arm the install-all fallback. Sole static reference to `js_nm_install_all`.
/// Emitted by codegen at dynamic require / getBuiltinModule lowering sites.
#[no_mangle]
pub extern "C" fn js_nm_enable_install_all() {
    // `black_box` hides that the stored value is `js_nm_install_all`. Without it,
    // whole-program optimization proves this is the only value ever written to
    // the single-pointer NM_INSTALL_ALL_HOOK and speculatively DEVIRTUALIZES the
    // indirect call in `nm_run_install_all_hook` into a direct `js_nm_install_all`
    // reference — re-pinning every bucket in programs that merely LINK the
    // resolver (every program, via the process method table). The opaque pointer
    // keeps the call genuinely indirect, so `js_nm_install_all` is reachable only
    // through THIS setter, which only getBuiltinModule/require callers emit.
    // (The per-bucket NM_DISPATCH_REGISTRY array is immune — its loads are indexed
    // by a runtime bucket id, so the optimizer can't pin a specific slot's fn.)
    let f = std::hint::black_box(js_nm_install_all as extern "C" fn());
    NM_INSTALL_ALL_HOOK.store(f as *mut (), Ordering::Relaxed);
}

/// Run the install-all fallback if armed. References only the pointer — never
/// `js_nm_install_all` directly — so it pins nothing on its own.
pub(crate) fn nm_run_install_all_hook() {
    let p = NM_INSTALL_ALL_HOOK.load(Ordering::Relaxed);
    if !p.is_null() {
        unsafe { std::mem::transmute::<*mut (), extern "C" fn()>(p)() };
    }
}

// ── Per-module constructor registry (devirt phase 2) ───────────────────────
// `new <namespace>.<Ctor>()` for node-module-namespaced constructors. Mirrors
// the method-dispatch registry: populated by `js_nm_install_<module>()` (only
// the 8 ctor-owning buckets register a fn), looked up by `js_new_function_construct`.
use super::class_registry::{
    nm_ctor_fs, nm_ctor_readline, nm_ctor_repl, nm_ctor_stream, nm_ctor_tls, nm_ctor_tty,
    nm_ctor_vm, nm_ctor_wasi,
};

type NmCtorFn = unsafe fn(&str, &str, *const f64, usize) -> Option<f64>;

static NM_CTOR_REGISTRY: [AtomicPtr<()>; NM_BUCKET_COUNT] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; NM_BUCKET_COUNT];

/// Look up the installed per-module constructor fn for `module`. `None` if the
/// module owns no namespaced constructors or its install was never emitted.
pub(crate) fn nm_ctor_lookup(module: &str) -> Option<NmCtorFn> {
    // `readline/promises` (and friends) bucket on the first path segment.
    let b = nm_module_index(module)
        .or_else(|| nm_module_index(module.split('/').next().unwrap_or(module)))?;
    let p = NM_CTOR_REGISTRY[b as usize].load(Ordering::Relaxed);
    if !p.is_null() {
        return Some(unsafe { std::mem::transmute::<*mut (), NmCtorFn>(p) });
    }
    // See nm_dispatch_lookup: unit tests construct directly without the codegen
    // install; lazily populate so tests exercise the real registry.
    #[cfg(test)]
    {
        js_nm_install_all();
        let p = NM_CTOR_REGISTRY[b as usize].load(Ordering::Relaxed);
        if !p.is_null() {
            return Some(unsafe { std::mem::transmute::<*mut (), NmCtorFn>(p) });
        }
    }
    None
}

/// Register a bucket's constructor fn. Called from the relevant
/// `js_nm_install_<module>()` so a ctor is reachable only when its module is
/// imported. `black_box` is unnecessary here (array slot indexed by a runtime
/// bucket id, like NM_DISPATCH_REGISTRY — not speculatively devirtualizable).
fn nm_register_ctor(b: NmBucket, f: NmCtorFn) {
    NM_CTOR_REGISTRY[b as usize].store(f as *mut (), Ordering::Relaxed);
}
