//! Native-module declaration ownership, shared by static export lowering and dynamic dispatch.
#[derive(Copy, Clone)]
#[repr(usize)]
pub enum NativeModuleBucket {
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
    WorkerThreads,
}
pub fn native_module_bucket(name: &str) -> Option<NativeModuleBucket> {
    match name {
        "assert" | "assert/strict" => Some(NativeModuleBucket::Assert),
        "async_hooks" => Some(NativeModuleBucket::AsyncHooks),
        "bigint" => Some(NativeModuleBucket::Bigint),
        "buffer" | "buffer.Buffer" => Some(NativeModuleBucket::Buffer),
        "bun" | "bun.ant" | "bun:jsc" => Some(NativeModuleBucket::Bun),
        // #6562: the `bun:` prefix is part of the name (not stripped like
        // `node:`).
        "bun:ffi" | "ffi" | "ffi.default" => Some(NativeModuleBucket::BunFfi),
        "child_process" => Some(NativeModuleBucket::ChildProcess),
        "cluster" => Some(NativeModuleBucket::Cluster),
        "console" => Some(NativeModuleBucket::Console),
        "crypto" | "crypto.Certificate" | "crypto.KeyObject" | "crypto.subtle"
        | "crypto.webcrypto" => Some(NativeModuleBucket::Crypto),
        "dgram" => Some(NativeModuleBucket::Dgram),
        "dns" | "dns/promises" => Some(NativeModuleBucket::Dns),
        "domain" => Some(NativeModuleBucket::Domain),
        "events" => Some(NativeModuleBucket::Events),
        "fs" => Some(NativeModuleBucket::Fs),
        "http" | "http2" | "https" => Some(NativeModuleBucket::Http),
        "inspector"
        | "inspector.Network"
        | "inspector.NetworkResources"
        | "inspector.DOMStorage"
        | "inspector.Session"
        | "inspector/promises"
        | "inspector/promises.Session" => Some(NativeModuleBucket::Inspector),
        "module" => Some(NativeModuleBucket::Module),
        "net" => Some(NativeModuleBucket::Net),
        // #6563: node-pty + the API-identical @lydell fork, one bucket.
        "node-pty" | "@lydell/node-pty" | "bun-pty" => Some(NativeModuleBucket::NodePty),
        "os" => Some(NativeModuleBucket::Os),
        "path" | "path.posix" | "path.win32" => Some(NativeModuleBucket::Path),
        "perf_histogram" | "perf_hooks" | "perf_observer" | "perf_observer_list" => {
            Some(NativeModuleBucket::Perf)
        }
        "process" => Some(NativeModuleBucket::Process),
        "punycode" | "punycode.ucs2" | "punycode.default" => Some(NativeModuleBucket::Punycode),
        "querystring" => Some(NativeModuleBucket::Querystring),
        "readline" => Some(NativeModuleBucket::Readline),
        "repl" => Some(NativeModuleBucket::Repl),
        "sea" => Some(NativeModuleBucket::Sea),
        "sqlite" => Some(NativeModuleBucket::Sqlite),
        "stream" => Some(NativeModuleBucket::Stream),
        "timers" => Some(NativeModuleBucket::Timers),
        "tls" => Some(NativeModuleBucket::Tls),
        "tty" => Some(NativeModuleBucket::Tty),
        "url" => Some(NativeModuleBucket::Url),
        "util" | "util.types" | "util/types" => Some(NativeModuleBucket::Util),
        "v8"
        | "v8.Deserializer"
        | "v8.GCProfiler"
        | "v8.Serializer"
        | "v8.promiseHooks"
        | "v8.startupSnapshot"
        | "v8.DefaultSerializer"
        | "v8.DefaultDeserializer" => Some(NativeModuleBucket::V8),
        "vm" => Some(NativeModuleBucket::Vm),
        "worker_threads" => Some(NativeModuleBucket::WorkerThreads),
        "wasi" => Some(NativeModuleBucket::Wasi),
        "zlib" => Some(NativeModuleBucket::Zlib),
        _ => None,
    }
}

impl NativeModuleBucket {
    pub fn dispatch_name(self) -> &'static str {
        match self {
            Self::Assert => "assert",
            Self::AsyncHooks => "async_hooks",
            Self::Bigint => "bigint",
            Self::Buffer => "buffer",
            Self::Bun => "bun",
            Self::BunFfi => "bun_ffi",
            Self::ChildProcess => "child_process",
            Self::Cluster => "cluster",
            Self::Console => "console",
            Self::Crypto => "crypto",
            Self::Dgram => "dgram",
            Self::Dns => "dns",
            Self::Domain => "domain",
            Self::Events => "events",
            Self::Fs => "fs",
            Self::Http => "http",
            Self::Inspector => "inspector",
            Self::Module => "module",
            Self::Net => "net",
            Self::NodePty => "node_pty",
            Self::Os => "os",
            Self::Path => "path",
            Self::Perf => "perf",
            Self::Process => "process",
            Self::Punycode => "punycode",
            Self::Querystring => "querystring",
            Self::Readline => "readline",
            Self::Repl => "repl",
            Self::Sea => "sea",
            Self::Sqlite => "sqlite",
            Self::Stream => "stream",
            Self::Timers => "timers",
            Self::Tls => "tls",
            Self::Tty => "tty",
            Self::Url => "url",
            Self::Util => "util",
            Self::V8 => "v8",
            Self::Vm => "vm",
            Self::Wasi => "wasi",
            Self::Zlib => "zlib",
            Self::WorkerThreads => "worker_threads",
        }
    }
}
