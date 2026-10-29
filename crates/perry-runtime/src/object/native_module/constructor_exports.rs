use super::*;

/// Constructor metadata for native-module callable exports.
///
/// Native exports and ordinary module functions share the same bound-method
/// closure trampoline, so callability alone cannot answer `IsConstructor`.
/// Keep the distinction explicit here instead of treating every export (or
/// every capitalized name) as constructable. Node exposes many lower-case
/// JavaScript wrapper functions that *are* constructable (`repl.start`,
/// `events.init`, most `fs` callbacks), so this is deliberately an exact
/// non-constructor metadata table rather than a capitalization heuristic.
pub(crate) fn is_native_module_constructor_export(module: &str, property: &str) -> bool {
    let module = normalize_native_module_alias(module);
    let module = cjs_default_base_module(module).unwrap_or(module);
    let module = assert_instance_base_module(module).unwrap_or(module);
    let module = normalize_native_module_alias(module);
    let property = canonical_native_callable_property(module, property);

    // Histogram constructors are only reachable through an instance's
    // `constructor` property.  They are callable-shaped internal exports, and
    // their construct path deliberately throws ERR_ILLEGAL_CONSTRUCTOR.
    if module == "perf_histogram" && matches!(property, "RecordableHistogram" | "ELDHistogram") {
        return true;
    }

    if !is_native_module_callable_export(module, property) {
        return false;
    }

    !crate::realm_decl! { native_nonconstructors(module, property);
        "assert" | "assert/strict" => [ "doesNotReject" | "rejects"],
        "bun" => [

            "build"
                | "file"
                | "fileURLToPath"
                | "hash"
                | "pathToFileURL"
                | "plugin"
                | "stringWidth"
                | "write"
        ],
        "buffer.Buffer" => ["of"],
        "console" => [

            "assert"
                | "clear"
                | "context"
                | "count"
                | "countReset"
                | "createTask"
                | "debug"
                | "dir"
                | "dirxml"
                | "error"
                | "group"
                | "groupCollapsed"
                | "groupEnd"
                | "info"
                | "log"
                | "profile"
                | "profileEnd"
                | "table"
                | "time"
                | "timeEnd"
                | "timeLog"
                | "timeStamp"
                | "trace"
                | "warn"
        ],
        "crypto" => [

            "getCiphers"
                | "getCurves"
                | "getHashes"
                | "privateDecrypt"
                | "privateEncrypt"
                | "publicDecrypt"
                | "publicEncrypt"
                | "timingSafeEqual"
        ],
        "crypto.KeyObject" => ["from"],
        "dns" | "dns/promises" => ["getServers"],
        "events" => ["once"],
        "http" => ["setMaxIdleHTTPParsers"],
        "inspector" => [ "close" | "url"],
        "inspector.DOMStorage" => [

            "domStorageItemAdded"
                | "domStorageItemRemoved"
                | "domStorageItemUpdated"
                | "domStorageItemsCleared"
                | "registerStorage"
        ],
        "inspector.Network" => [

            "dataReceived"
                | "dataSent"
                | "loadingFailed"
                | "loadingFinished"
                | "requestWillBeSent"
                | "responseReceived"
                | "webSocketClosed"
                | "webSocketCreated"
                | "webSocketHandshakeResponseReceived"
        ],
        "inspector.Session" | "inspector/promises.Session" => ["once"],
        "module" => [ "flushCompileCache" | "isBuiltin"],
        "os" => [

            "availableParallelism"
                | "freemem"
                | "machine"
                | "release"
                | "totalmem"
                | "type"
                | "version"
        ],
        "path" | "path.posix" | "path.win32" => [

            "basename"
                | "dirname"
                | "extname"
                | "isAbsolute"
                | "join"
                | "matchesGlob"
                | "normalize"
                | "parse"
                | "relative"
                | "resolve"
                | "toNamespacedPath"
        ],
        "process" => [

            "_debugEnd"
                | "_debugProcess"
                | "_fatalException"
                | "_getActiveHandles"
                | "_getActiveRequests"
                | "_kill"
                | "_startProfilerIdleNotifier"
                | "_stopProfilerIdleNotifier"
                | "abort"
                | "availableMemory"
                | "constrainedMemory"
                | "dlopen"
                | "getActiveResourcesInfo"
                | "getegid"
                | "geteuid"
                | "getgid"
                | "getgroups"
                | "getuid"
                | "reallyExit"
                | "uptime"
        ],
        "punycode.ucs2" => ["encode"],
        "stream" => ["_isArrayBufferView"],
        "timers/promises" => ["setInterval"],
        "tls" => ["getCiphers"],
        "util" => [

            "aborted" | "isDeepStrictEqual" | "parseArgs" | "toUSVString"
        ],
        "util.types" | "util/types" => [

            "isAnyArrayBuffer"
                | "isArgumentsObject"
                | "isArrayBuffer"
                | "isArrayBufferView"
                | "isAsyncFunction"
                | "isBigIntObject"
                | "isBooleanObject"
                | "isBoxedPrimitive"
                | "isCryptoKey"
                | "isDate"
                | "isExternal"
                | "isGeneratorFunction"
                | "isGeneratorObject"
                | "isKeyObject"
                | "isMap"
                | "isMapIterator"
                | "isModuleNamespaceObject"
                | "isNativeError"
                | "isNumberObject"
                | "isPromise"
                | "isProxy"
                | "isRegExp"
                | "isSet"
                | "isSetIterator"
                | "isSharedArrayBuffer"
                | "isStringObject"
                | "isSymbolObject"
                | "isWeakMap"
                | "isWeakSet"
        ],
        "v8" => [

            "cachedDataVersionTag" | "stopCoverage" | "takeCoverage"
        ],
        "v8.promiseHooks" => [ "onAfter" | "onBefore" | "onInit" | "onSettled"],
        "worker_threads" => [ "moveMessagePortToContext" | "postMessageToThread"],

    }
}

pub(crate) fn bound_native_callable_is_constructor_value(value: f64) -> bool {
    unsafe { bound_native_callable_module_and_method(value) }
        .is_some_and(|(module, property)| is_native_module_constructor_export(&module, &property))
}

#[cfg(test)]
mod tests {
    use super::is_native_module_constructor_export;

    #[test]
    fn fs_read_file_and_stream_classes_expose_prototypes() {
        assert!(is_native_module_constructor_export("fs", "readFile"));
        assert!(is_native_module_constructor_export("fs", "ReadStream"));
        assert!(is_native_module_constructor_export(
            "node:fs",
            "WriteStream"
        ));
    }

    #[test]
    fn histogram_class_values_are_constructor_shaped() {
        assert!(is_native_module_constructor_export(
            "perf_histogram",
            "RecordableHistogram"
        ));
        assert!(is_native_module_constructor_export(
            "perf_histogram",
            "ELDHistogram"
        ));
    }
}
