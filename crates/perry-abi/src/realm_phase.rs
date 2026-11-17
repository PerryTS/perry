//! The two phases of the realm global's population, as one declaration.
//!
//! The realm global is born on a static layout: every builtin key and its
//! attributes are declared before anything is bound. The population binds
//! the declared slots in two phases. Phase A, the core, is what a program
//! pays at its first touch of the realm; phase B, the rest, runs before
//! anything can observe one of its slots: the global object as a value, a
//! name of [`REALM_REST_GLOBALS`], an unknown name, any store.
//!
//! Both sides read this file and nothing else decides a phase:
//! * the runtime binds a declared name in phase B iff [`realm_rest_global`]
//!   (`perry-runtime`, `object/global_this/populate.rs`);
//! * the compiler reads a named global through the core entry
//!   (`js_get_global_core`) iff [`realm_core_global`], and through the full
//!   entry (`js_get_global_this`) otherwise (`perry-codegen`,
//!   `expr/property_get/globalget.rs`).
//!
//! The three lists partition the global's declared keys (a runtime test,
//! `two_phase_tests::phase_lists_partition_the_declared_keys`). The phase of
//! a name is no part of its `#[realm]` declaration: the realm table and the
//! facts built from it do not know when a slot is bound.

/// One list of names, three ways: the slice (tests, the compiler's audit),
/// the names joined by spaces (a loop over them in the runtime: one string,
/// no table of relocated pointers in the binary) and the membership test (a
/// `match` on the name: code, no table either).
macro_rules! realm_names {
    ($(#[$doc:meta])* $list:ident, $joined:ident, $(#[$member_attr:meta])* $member:ident: $($name:literal)*) => {
        $(#[$doc])*
        pub const $list: &[&str] = &[$($name),*];
        /// The same names in the same order, each followed by one space.
        pub const $joined: &str = concat!($($name, " "),*);
        /// Is `name` one of them?
        $(#[$member_attr])*
        pub fn $member(name: &str) -> bool {
            matches!(name, $($name)|*)
        }
    };
}

realm_names! {
    /// Declared data names phase B binds. Reading one, by any path, runs
    /// phase B first. `Date` (the costliest single family of the population,
    /// and one no program reaches without naming it or asking one of its
    /// instances for a property), the Event, DOMException and abort families,
    /// and, the last sixteen, the byte-cell families: the typed
    /// arrays (with the shared `%TypedArray%` intrinsic, built with the first
    /// of them), the buffers, `DataView`, and node's `Buffer` (a `Uint8Array`
    /// subclass).
    REALM_REST_GLOBALS, REALM_REST_NAMES,
    // Only a phase of a two-phase realm asks (the runtime's `.perry_tail`).
    #[cfg_attr(target_os = "linux", link_section = ".perry_tail.realm_rest_global")]
    #[inline(never)]
    realm_rest_global:
        "WeakMap" "WeakSet" "WeakRef" "Proxy" "FinalizationRegistry" "Atomics" "Intl"
        "WebAssembly" "Temporal" "PerformanceEntry" "PerformanceMark" "PerformanceMeasure"
        "PerformanceObserver" "PerformanceObserverEntryList" "PerformanceResourceTiming" "Date"
        "DOMException" "Event" "CustomEvent" "MessageEvent" "AbortSignal" "AbortController"
        "Uint8Array" "Int8Array" "Uint16Array" "Int16Array" "Uint32Array" "Int32Array"
        "Float16Array" "Float32Array" "Float64Array" "Uint8ClampedArray" "BigInt64Array"
        "BigUint64Array" "ArrayBuffer" "SharedArrayBuffer" "DataView" "Buffer"
}

realm_names! {
    /// Declared names phase A binds that are never read through the core
    /// entry: the global object itself (a value that shows every slot) and
    /// the accessors (their getters run with the global as `this`).
    REALM_VALUE_GLOBALS, REALM_VALUE_NAMES, realm_value_global:
        "globalThis" "global" "localStorage" "sessionStorage" "crypto" "navigator"
}

realm_names! {
    /// Declared data names phase A binds: the only names a compiled static
    /// read may reach through the core entry (`js_get_global_core`). Every
    /// other name (phase B's, the global itself, an accessor, a name the
    /// layout does not declare) is read through the full entry.
    REALM_CORE_GLOBALS, REALM_CORE_NAMES, realm_core_global:
        "Array" "Object" "String" "Number" "Boolean" "Function" "RegExp" "Error" "TypeError"
        "RangeError" "SyntaxError" "ReferenceError" "EvalError" "URIError" "AggregateError"
        "Symbol" "Promise" "Map" "Set" "BigInt" "TextEncoder" "TextDecoder" "TextEncoderStream"
        "TextDecoderStream" "CompressionStream" "DecompressionStream" "ReadableStream"
        "ReadableStreamDefaultController" "ReadableStreamDefaultReader"
        "ReadableByteStreamController" "ReadableStreamBYOBReader" "ReadableStreamBYOBRequest"
        "WritableStream" "TransformStream" "Navigator" "URL" "URLSearchParams" "URLPattern"
        "EventTarget" "Crypto" "CryptoKey" "SubtleCrypto"
        "FormData" "Blob" "File" "Headers"
        "Request" "Response" "MessageChannel" "MessagePort" "BroadcastChannel" "Storage"
        "WebSocket" "DisposableStack" "AsyncDisposableStack" "SuppressedError" "Iterator" "eval"
        "fetch" "structuredClone" "atob" "btoa" "setTimeout" "clearTimeout" "setInterval"
        "clearInterval" "setImmediate" "clearImmediate" "queueMicrotask" "gc" "parseInt"
        "parseFloat" "isNaN" "isFinite" "encodeURI" "decodeURI" "encodeURIComponent"
        "decodeURIComponent" "escape" "unescape" "console" "process" "Math" "JSON" "Reflect"
        "performance" "Performance" "NaN" "Infinity" "undefined"
}

/// The word a phase-B slot holds from the global's birth until phase B binds
/// it, in a program compiled in check mode (`PERRY_VERIFY_FACTS=1`): a word of
/// the runtime-internal `0x7FFE` space with a payload of its own. It is no JS
/// value, not `undefined`, and not the hole or the absent marker, so a reader
/// that sees it can only have skipped phase B: check mode reports that read
/// and exits with 86. Outside check mode the slot is born `undefined`.
pub const REALM_UNBOUND_POISON: u64 = 0x7FFE_0000_B0B0_0B0B;
