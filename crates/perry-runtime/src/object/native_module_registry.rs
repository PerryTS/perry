//! Per-module native-module method-dispatch registry (devirtualization).
//! GENERATED scaffolding — see NM_DEVIRT_PLAN.md. Each `js_nm_install_<m>()` is
//! the SOLE static reference to its `nm_dispatch_<m>` bucket fn; codegen emits a
//! call per statically-imported native module so the linker dead-strips the rest.
//! NOTHING here names all buckets together (that would re-pin everything).
use super::native_module_dispatch::{NmCtx, nm_dispatch_assert, nm_dispatch_async_hooks, nm_dispatch_bigint, nm_dispatch_buffer, nm_dispatch_child_process, nm_dispatch_cluster, nm_dispatch_console, nm_dispatch_crypto, nm_dispatch_dgram, nm_dispatch_dns, nm_dispatch_domain, nm_dispatch_events, nm_dispatch_fs, nm_dispatch_http, nm_dispatch_inspector, nm_dispatch_module, nm_dispatch_net, nm_dispatch_os, nm_dispatch_path, nm_dispatch_perf, nm_dispatch_process, nm_dispatch_punycode, nm_dispatch_querystring, nm_dispatch_readline, nm_dispatch_repl, nm_dispatch_sea, nm_dispatch_sqlite, nm_dispatch_stream, nm_dispatch_timers, nm_dispatch_tls, nm_dispatch_tty, nm_dispatch_url, nm_dispatch_util, nm_dispatch_v8, nm_dispatch_vm, nm_dispatch_wasi, nm_dispatch_zlib};
use std::sync::atomic::{AtomicPtr, Ordering};

type NmDispatchFn = unsafe fn(&NmCtx, &str, &str) -> f64;

#[derive(Copy, Clone)]
#[repr(usize)]
enum NmBucket { Assert, AsyncHooks, Bigint, Buffer, ChildProcess, Cluster, Console, Crypto, Dgram, Dns, Domain, Events, Fs, Http, Inspector, Module, Net, Os, Path, Perf, Process, Punycode, Querystring, Readline, Repl, Sea, Sqlite, Stream, Timers, Tls, Tty, Url, Util, V8, Vm, Wasi, Zlib, }
const NM_BUCKET_COUNT: usize = 37;

static NM_DISPATCH_REGISTRY: [AtomicPtr<()>; NM_BUCKET_COUNT] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; NM_BUCKET_COUNT];

/// Map a (normalized) module tag to its bucket. Pure string match — references
/// no bucket fn, so it pins nothing.
fn nm_module_index(name: &str) -> Option<NmBucket> {
    match name {
        "assert" => Some(NmBucket::Assert),
        "async_hooks" => Some(NmBucket::AsyncHooks),
        "bigint" => Some(NmBucket::Bigint),
        "buffer" | "buffer.Buffer" => Some(NmBucket::Buffer),
        "child_process" => Some(NmBucket::ChildProcess),
        "cluster" => Some(NmBucket::Cluster),
        "console" => Some(NmBucket::Console),
        "crypto" | "crypto.Certificate" | "crypto.KeyObject" | "crypto.subtle" | "crypto.webcrypto" => Some(NmBucket::Crypto),
        "dgram" => Some(NmBucket::Dgram),
        "dns" | "dns/promises" => Some(NmBucket::Dns),
        "domain" => Some(NmBucket::Domain),
        "events" => Some(NmBucket::Events),
        "fs" => Some(NmBucket::Fs),
        "http" => Some(NmBucket::Http),
        "inspector" | "inspector.Network" | "inspector/promises" => Some(NmBucket::Inspector),
        "module" => Some(NmBucket::Module),
        "net" => Some(NmBucket::Net),
        "os" => Some(NmBucket::Os),
        "path" | "path.posix" | "path.win32" => Some(NmBucket::Path),
        "perf_histogram" | "perf_hooks" | "perf_observer" | "perf_observer_list" => Some(NmBucket::Perf),
        "process" => Some(NmBucket::Process),
        "punycode" | "punycode.ucs2" => Some(NmBucket::Punycode),
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
        "v8" | "v8.Deserializer" | "v8.GCProfiler" | "v8.Serializer" | "v8.promiseHooks" | "v8.startupSnapshot" => Some(NmBucket::V8),
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
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), NmDispatchFn>(p) })
    }
}

#[no_mangle]
pub extern "C" fn js_nm_install_assert() { NM_DISPATCH_REGISTRY[NmBucket::Assert as usize].store(nm_dispatch_assert as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_async_hooks() { NM_DISPATCH_REGISTRY[NmBucket::AsyncHooks as usize].store(nm_dispatch_async_hooks as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_bigint() { NM_DISPATCH_REGISTRY[NmBucket::Bigint as usize].store(nm_dispatch_bigint as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_buffer() { NM_DISPATCH_REGISTRY[NmBucket::Buffer as usize].store(nm_dispatch_buffer as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_child_process() { NM_DISPATCH_REGISTRY[NmBucket::ChildProcess as usize].store(nm_dispatch_child_process as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_cluster() { NM_DISPATCH_REGISTRY[NmBucket::Cluster as usize].store(nm_dispatch_cluster as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_console() { NM_DISPATCH_REGISTRY[NmBucket::Console as usize].store(nm_dispatch_console as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_crypto() { NM_DISPATCH_REGISTRY[NmBucket::Crypto as usize].store(nm_dispatch_crypto as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_dgram() { NM_DISPATCH_REGISTRY[NmBucket::Dgram as usize].store(nm_dispatch_dgram as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_dns() { NM_DISPATCH_REGISTRY[NmBucket::Dns as usize].store(nm_dispatch_dns as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_domain() { NM_DISPATCH_REGISTRY[NmBucket::Domain as usize].store(nm_dispatch_domain as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_events() { NM_DISPATCH_REGISTRY[NmBucket::Events as usize].store(nm_dispatch_events as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_fs() { NM_DISPATCH_REGISTRY[NmBucket::Fs as usize].store(nm_dispatch_fs as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_http() { NM_DISPATCH_REGISTRY[NmBucket::Http as usize].store(nm_dispatch_http as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_inspector() { NM_DISPATCH_REGISTRY[NmBucket::Inspector as usize].store(nm_dispatch_inspector as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_module() { NM_DISPATCH_REGISTRY[NmBucket::Module as usize].store(nm_dispatch_module as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_net() { NM_DISPATCH_REGISTRY[NmBucket::Net as usize].store(nm_dispatch_net as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_os() { NM_DISPATCH_REGISTRY[NmBucket::Os as usize].store(nm_dispatch_os as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_path() { NM_DISPATCH_REGISTRY[NmBucket::Path as usize].store(nm_dispatch_path as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_perf() { NM_DISPATCH_REGISTRY[NmBucket::Perf as usize].store(nm_dispatch_perf as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_process() { NM_DISPATCH_REGISTRY[NmBucket::Process as usize].store(nm_dispatch_process as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_punycode() { NM_DISPATCH_REGISTRY[NmBucket::Punycode as usize].store(nm_dispatch_punycode as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_querystring() { NM_DISPATCH_REGISTRY[NmBucket::Querystring as usize].store(nm_dispatch_querystring as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_readline() { NM_DISPATCH_REGISTRY[NmBucket::Readline as usize].store(nm_dispatch_readline as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_repl() { NM_DISPATCH_REGISTRY[NmBucket::Repl as usize].store(nm_dispatch_repl as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_sea() { NM_DISPATCH_REGISTRY[NmBucket::Sea as usize].store(nm_dispatch_sea as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_sqlite() { NM_DISPATCH_REGISTRY[NmBucket::Sqlite as usize].store(nm_dispatch_sqlite as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_stream() { NM_DISPATCH_REGISTRY[NmBucket::Stream as usize].store(nm_dispatch_stream as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_timers() { NM_DISPATCH_REGISTRY[NmBucket::Timers as usize].store(nm_dispatch_timers as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_tls() { NM_DISPATCH_REGISTRY[NmBucket::Tls as usize].store(nm_dispatch_tls as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_tty() { NM_DISPATCH_REGISTRY[NmBucket::Tty as usize].store(nm_dispatch_tty as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_url() { NM_DISPATCH_REGISTRY[NmBucket::Url as usize].store(nm_dispatch_url as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_util() { NM_DISPATCH_REGISTRY[NmBucket::Util as usize].store(nm_dispatch_util as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_v8() { NM_DISPATCH_REGISTRY[NmBucket::V8 as usize].store(nm_dispatch_v8 as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_vm() { NM_DISPATCH_REGISTRY[NmBucket::Vm as usize].store(nm_dispatch_vm as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_wasi() { NM_DISPATCH_REGISTRY[NmBucket::Wasi as usize].store(nm_dispatch_wasi as NmDispatchFn as *mut (), Ordering::Relaxed); }
#[no_mangle]
pub extern "C" fn js_nm_install_zlib() { NM_DISPATCH_REGISTRY[NmBucket::Zlib as usize].store(nm_dispatch_zlib as NmDispatchFn as *mut (), Ordering::Relaxed); }

/// Dynamic-require fallback: register every bucket. Emitted by codegen only when
/// a native module is imported under an unanalyzable (dynamic) name.
#[no_mangle]
pub extern "C" fn js_nm_install_all() {
    js_nm_install_assert();
    js_nm_install_async_hooks();
    js_nm_install_bigint();
    js_nm_install_buffer();
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
