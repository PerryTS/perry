//! Pure module-name tables for the namespace machinery: callable-property
//! aliases, `assert` instance-module bases, and which namespaces are cached as
//! one object per module. Split out of `object/native_module.rs` to stay under
//! the 2,000-line cap (#10750).

pub(crate) fn canonical_native_callable_property<'a>(
    module_name: &str,
    property_name: &'a str,
) -> &'a str {
    match (module_name, property_name) {
        ("fs", "FileReadStream") => "ReadStream",
        ("fs", "FileWriteStream") => "WriteStream",
        ("path" | "path.posix" | "path.win32", "_makeLong") => "toNamespacedPath",
        ("querystring", "decode") => "parse",
        ("querystring", "encode") => "stringify",
        ("cluster", "setupMaster") => "setupPrimary",
        _ => property_name,
    }
}

pub(crate) fn assert_instance_base_module(module_name: &str) -> Option<&'static str> {
    match module_name {
        "assert.instance" | "assert.instance.skip" => Some("assert"),
        "assert/strict.instance" | "assert/strict.instance.skip" => Some("assert/strict"),
        _ => None,
    }
}

pub(super) fn should_cache_native_module_namespace(module_name: &str) -> bool {
    matches!(
        module_name,
        "assert/strict"
            | "async_hooks"
            | "async_hooks.default"
            | "bun"
            | "bun.ant"
            | "constants"
            | "constants.default"
            // #5263: cache the top-level namespace objects whose dynamic
            // member access is now allowed by default. A stable (cached)
            // namespace object means a user-set symbol property
            // (`fs[Symbol.for('graceful-fs.queue')] = queue`, keyed by object
            // pointer in `SYMBOL_PROPERTIES`) round-trips on reads — otherwise
            // each `NativeModuleRef` mints a fresh object and the write is lost.
            // String-keyed writes already persist via the module-keyed
            // `NATIVE_NAMESPACE_PROP_OVERRIDES` side-table. These are pure
            // tag+name holders (all real dispatch keys off the module name, not
            // object state), so caching only affects object identity.
            | "fs"
            // #10428: one object per module, so `require('node:http') === require('http')`.
            | "http"
            | "https"
            | "http2"
            | "net"
            | "dns.default"
            | "dns/promises.default"
            | "child_process.default"
            | "cluster"
            | "cluster.default"
            | "dgram"
            | "events"
            | "fs.constants"
            | "inspector"
            | "inspector.default"
            | "inspector.Network"
            | "inspector/promises"
            | "inspector/promises.default"
            | "module"
            | "node-pty"
            | "node-pty.default"
            | "os"
            | "os.default"
            | "path"
            | "path.default"
            | "path.posix.default"
            | "path.win32.default"
            | "perf_hooks.default"
            | "punycode"
            | "punycode.default"
            | "punycode.ucs2"
            | "querystring"
            | "querystring.default"
            | "repl"
            | "repl.default"
            | "sea"
            | "sea.default"
            | "process"
            | "process.namespace"
            | "process.default"
            | "url"
            | "url.default"
            | "util"
            | "util.default"
            | "util.types"
            | "path.posix"
            | "path.win32"
            | "readline/promises"
            | "timers/promises"
            | "vm"
            | "vm.constants"
            | "crypto.webcrypto"
            | "crypto.subtle"
    )
}
