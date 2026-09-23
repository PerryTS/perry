//! Whole-program proof that a program can never schedule asynchronous work
//! (binary size).
//!
//! The entry module's `main` normally ends in the event loop: microtask
//! drains, timer phases, the stdlib pump, `beforeExit`, the park in
//! `js_wait_for_event`. Those calls are the only references to the microtask
//! runner and its satellites (~160 KB in every binary), so a program that
//! provably has nothing to run after its top level can end with the exit
//! sequence alone.
//!
//! The proof is an ALLOWLIST, never a denylist: every construct in every
//! module must be one this file knows to be synchronous, and every global
//! read by name must be a known-synchronous global. A construct this file has
//! never heard of — including every HIR variant added after it was written —
//! keeps the event loop. Getting it wrong in that direction costs bytes;
//! getting it wrong in the other would exit before callbacks ran, so every
//! doubt resolves to "keep the loop".
//!
//! The check reads the HIR's `Debug` rendering: string literals are pulled
//! out first (their contents are data, not constructs), then every remaining
//! capitalised identifier — a variant or struct name — must be allowlisted.

use std::collections::BTreeSet;

use super::CompilationContext;

/// HIR variant / struct / type names that can only run synchronous code.
///
/// Deliberately absent: `Await`, anything `Async*`, promises, timers,
/// `QueueMicrotask`, `Process{NextTick,On,Once,EmitWarning,Stdin*,Stdout*,
/// Stderr*}`, `FinalizationRegistry*`, `WebAssembly*`, `WebCrypto*`, `Fetch*`,
/// `Net*`, `ChildProcess*`, `Worker*`, `DynamicImport`, `Js*` (V8 interop),
/// `NativeModuleRef`, `WithGet`/`WithSet`, `GetAsyncIterator`,
/// `ForAwaitToArray`.
const SYNC_NAMES: &[&str] = &[
    // structure
    "Module",
    "Function",
    "Class",
    "ClassField",
    "ClassMethod",
    "ClassAccessor",
    "ClassStaticBlock",
    "Param",
    "Some",
    "None",
    "Ok",
    "Err",
    "Box",
    "Vec",
    "CatchClause",
    "SwitchCase",
    "PropertyInfo",
    "Named",
    "ObjectType",
    "Generic",
    "Any",
    "Unknown",
    "Never",
    "Void",
    "Null",
    "Undefined",
    "Number",
    "String",
    "Boolean",
    "Bool",
    "Array",
    "Tuple",
    "Union",
    "Object",
    "Integer",
    "BigInt",
    "Symbol",
    "Float",
    "Int32",
    "Double",
    "Literal",
    "Import",
    "ImportSpecifier",
    "Export",
    "ExportSpecifier",
    "ModuleKind",
    "NativeCompiled",
    "Default",
    "Local",
    "Global",
    "Getter",
    "Setter",
    "Method",
    "Constructor",
    "Static",
    "Instance",
    "Private",
    "Public",
    "Protected",
    "Readonly",
    "Optional",
    "Required",
    "Rest",
    "Spread",
    "Eager",
    "FunctionSourceMetadata",
    "LocalSourceSpan",
    "Interface",
    "TypeAlias",
    "Enum",
    "EnumValue",
    "TypeParam",
    "TypeVar",
    "StringLiteral",
    "InterfaceProperty",
    "InterfaceMethod",
    // operator kinds (Binary / Compare / Unary / Logical / Update)
    "Add",
    "Sub",
    "Mul",
    "Div",
    "Mod",
    "Pow",
    "BitAnd",
    "BitOr",
    "BitXor",
    "Shl",
    "Shr",
    "UShr",
    "Eq",
    "Ne",
    "LooseEq",
    "LooseNe",
    "Lt",
    "Le",
    "Gt",
    "Ge",
    "Neg",
    "Not",
    "BitNot",
    "Pos",
    "And",
    "Or",
    "Coalesce",
    "Increment",
    "Decrement",
    // statements
    "Let",
    "Expr",
    "Return",
    "If",
    "While",
    "DoWhile",
    "For",
    "Labeled",
    "Break",
    "Continue",
    "LabeledBreak",
    "LabeledContinue",
    "Throw",
    "Try",
    "Switch",
    "PreallocateBoxes",
    "PreallocateTdzBoxes",
    "ReleaseBoxes",
    // core expressions
    "LocalGet",
    "LocalSet",
    "GlobalGet",
    "GlobalSet",
    "PropertyGet",
    "PropertySet",
    "PropertyUpdate",
    "IndexGet",
    "IndexSet",
    "IndexUpdate",
    "Binary",
    "Unary",
    "Compare",
    "Logical",
    "Conditional",
    "Call",
    "New",
    "Closure",
    "FuncRef",
    "ClassRef",
    "This",
    "SuperCall",
    "SuperMethodCall",
    "SuperPropertyGet",
    "Template",
    "TypeOf",
    "InstanceOf",
    "In",
    "Update",
    "Sequence",
    "Assign",
    "Delete",
    "Void",
    "StringCoerce",
    "NumberCoerce",
    "BooleanCoerce",
    "ObjectLiteral",
    "ArrayLiteral",
    "GetIterator",
    "ForOfToArray",
    "ForInKeys",
    "TaggedTemplateStrings",
    "Yield",
    "LinkGeneratorPrototype",
    "EnumMember",
    "StaticFieldGet",
    "StaticFieldSet",
    "StaticMethodCall",
    "PrivateBrandCheck",
    "PrivateGuard",
    "PutValueSet",
    "ModuleTopThis",
    "ImportMetaUrl",
    "WtfString",
    "I18nString",
    "RegisterClassParentDynamic",
    "RegisterClassCaptures",
    "RefreshClassExprCaptures",
    "RegisterClassStaticSymbol",
    "RegisterClassComputedMethod",
    "RegisterClassComputedAccessor",
    "RegisterPrototypeMethod",
    "RegisterFunctionPrototypeMethod",
    "GetFunctionPrototypeMethod",
    "ExternFuncRef",
    "NativeMethodCall",
    "ParseInt",
    "ParseFloat",
    "IsNaN",
    "IsFinite",
    "IsUndefinedOrBareNan",
    "EncodeURI",
    "DecodeURI",
    "EncodeURIComponent",
    "DecodeURIComponent",
    "Atob",
    "Btoa",
    "BoxedPrimitiveNew",
    "ErrorNew",
    "TypeErrorNew",
    "RangeErrorNew",
    "ReferenceErrorNew",
    "SyntaxErrorNew",
    "AggregateErrorNew",
    "EnvGet",
    "EnvGetDynamic",
    "ProcessEnv",
    "ProcessArgv",
    "ProcessExit",
    "ProcessPid",
    "ProcessPpid",
    "ProcessVersion",
    "ProcessVersions",
    "ProcessCwd",
    "ProcessUptime",
    "ProcessHrtime",
    "ProcessHrtimeBigint",
    "ProcessMemoryUsage",
    "PerformanceNow",
];

/// Name prefixes of HIR families whose members are all synchronous
/// (built-in object methods lowered to dedicated variants).
const SYNC_PREFIXES: &[&str] = &[
    "Array",
    "String",
    "Str",
    "Math",
    "Json",
    "Number",
    "Date",
    "Map",
    "Set",
    "Object",
    "Error",
    "BigInt",
    "Symbol",
    "Regex",
    "RegExp",
    "Iterator",
    "Weak",
    "Os",
    "Path",
    "Uint8Array",
    "TypedArray",
    "Buffer",
    "TextEncoder",
    "TextDecoder",
    "Url",
    "Proxy",
    "Reflect",
    "Crypto",
];

/// Substrings that disqualify a name even when a prefix above matches
/// (`SetTimeout`, `ArrayFromAsync`, `CryptoSubtle…`, `MapAsync…`).
const ASYNC_MARKERS: &[&str] = &[
    "Async",
    "Await",
    "Promise",
    "Timeout",
    "Interval",
    "Immediate",
    "Microtask",
    "Tick",
    "Fetch",
    "Worker",
    "Stream",
    "Socket",
    "Server",
    "Subtle",
    "Watch",
    "Signal",
    "Abort",
    "Listener",
    "Callback",
    "Spawn",
    "Fork",
    "Exec",
    "Finalization",
    "WebCrypto",
    "WebAssembly",
];

/// Globals that may be read by name (`GlobalGet` receivers collapse to a
/// sentinel, so the property name is all the HIR keeps).
const SYNC_GLOBAL_PROPS: &[&str] = &[
    "log",
    "error",
    "warn",
    "info",
    "debug",
    "trace",
    "table",
    "dir",
    "assert",
    "count",
    "countReset",
    "group",
    "groupCollapsed",
    "groupEnd",
    "Math",
    "JSON",
    "Number",
    "String",
    "Boolean",
    "Array",
    "Object",
    "Symbol",
    "BigInt",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "Date",
    "RegExp",
    "Error",
    "TypeError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "EvalError",
    "URIError",
    "AggregateError",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "Infinity",
    "NaN",
    "undefined",
    "encodeURIComponent",
    "decodeURIComponent",
    "encodeURI",
    "decodeURI",
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
    "ArrayBuffer",
    "DataView",
];

/// Runtime externs the HIR names directly that are synchronous.
const SYNC_EXTERNS: &[&str] = &[
    "js_for_of_next",
    "js_process_exit_code_get",
    "js_process_exit_code_set",
];

/// `NativeMethodCall` modules whose methods are all synchronous.
const SYNC_NATIVE_METHOD_MODULES: &[&str] = &[
    "array", "string", "map", "set", "math", "number", "object", "json", "date", "regexp",
    "symbol", "bigint", "error", "weakmap", "weakset",
];

/// Why a program is not proven synchronous, or `None` when it is.
pub(crate) fn why_not_synchronous(ctx: &CompilationContext) -> Option<String> {
    if ctx.needs_stdlib
        || ctx.needs_ui
        || ctx.needs_thread
        || ctx.needs_plugins
        || ctx.needs_geisterhand
        || ctx.needs_wasm_runtime
        || ctx.uses_fetch
    {
        return Some("stdlib/ui/thread/plugin/wasm/fetch in use".into());
    }
    if !ctx.native_addons.is_empty() || !ctx.native_module_imports.is_empty() {
        return Some("native modules imported".into());
    }
    if !ctx.js_modules.is_empty() {
        return Some("JavaScript modules in the graph".into());
    }
    if perry_hir::has_deferred_dynamic_code_sites() {
        return Some("dynamic code (eval / new Function)".into());
    }
    for (path, module) in &ctx.native_modules {
        if let Some(reason) = why_module_not_synchronous(&format!("{module:?}")) {
            return Some(format!("{}: {reason}", path.display()));
        }
    }
    None
}

pub(crate) fn program_is_synchronous(ctx: &CompilationContext) -> bool {
    let verdict = why_not_synchronous(ctx);
    if std::env::var_os("PERRY_SYNC_PROGRAM_DIAG").is_some() {
        match &verdict {
            None => eprintln!("[sync-program] proven synchronous: event loop omitted"),
            Some(reason) => eprintln!("[sync-program] keeps the event loop: {reason}"),
        }
    }
    verdict.is_none()
}

/// `constructor: Some(Function { id: 7, name: <here>` — a class constructor.
fn is_class_constructor_name(before: &str) -> bool {
    let Some(head) = before.strip_suffix(", name: ") else {
        return false;
    };
    let digits = head.trim_end_matches(|c: char| c.is_ascii_digit());
    digits.len() < head.len() && digits.ends_with("constructor: Some(Function { id: ")
}

/// Is the literal that follows `before` the property of a `GlobalGet(n)`
/// receiver (`PropertyGet { object: GlobalGet(3), property: <here>`)?
fn global_property_context(before: &str) -> bool {
    let Some(head) = before.strip_suffix("), property: ") else {
        return false;
    };
    let digits = head.trim_end_matches(|c: char| c.is_ascii_digit());
    digits.len() < head.len() && digits.ends_with("GlobalGet(")
}

fn is_sync_name(name: &str) -> bool {
    if ASYNC_MARKERS.iter().any(|marker| name.contains(marker)) {
        return false;
    }
    SYNC_NAMES.contains(&name) || SYNC_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// The per-module half of the proof, over the HIR `Debug` text.
pub(crate) fn why_module_not_synchronous(debug: &str) -> Option<String> {
    if debug.contains("is_async: true") {
        return Some("async function".into());
    }
    if debug.contains("references_global_this: true") {
        return Some("references globalThis".into());
    }
    // Quoted strings: record the ones that carry meaning, then drop them so
    // literal *data* is never mistaken for a construct.
    let mut rest = String::with_capacity(debug.len());
    let mut literals: Vec<(usize, String)> = Vec::new();
    let bytes = debug.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'"' {
                if bytes[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            literals.push((rest.len(), debug[start..j.min(bytes.len())].to_string()));
            rest.push_str("\"\"");
            i = j + 1;
        } else {
            let ch = debug[i..].chars().next().unwrap();
            rest.push(ch);
            i += ch.len_utf8();
        }
    }
    // The Function-constructor and eval escape hatches reach every global by
    // name at run time.
    for (pos, lit) in &literals {
        if matches!(
            lit.as_str(),
            "constructor" | "eval" | "Function" | "globalThis"
        ) {
            let before = &rest[..*pos];
            // A class's own constructor carries the name "constructor"; that
            // is structure, not a property read.
            if lit == "constructor" && is_class_constructor_name(before) {
                continue;
            }
            let context = &before[before.len().saturating_sub(80)..];
            return Some(format!("string {lit:?} after `{context}`"));
        }
    }
    // Context-sensitive literals: the text just before each literal says
    // which field it fills.
    for (pos, lit) in &literals {
        let before = &rest[..*pos];
        if before.ends_with("ExternFuncRef { name: ") && !SYNC_EXTERNS.contains(&lit.as_str()) {
            return Some(format!("extern {lit}"));
        }
        if before.ends_with("NativeMethodCall { module: ")
            && !SYNC_NATIVE_METHOD_MODULES.contains(&lit.as_str())
            && lit != "__perry_runtime"
        {
            return Some(format!("native method module {lit}"));
        }
        if global_property_context(before) && !SYNC_GLOBAL_PROPS.contains(&lit.as_str()) {
            return Some(format!("global {lit}"));
        }
    }
    // `__perry_runtime` native methods: only the synchronous iterator helpers.
    let marker = "NativeMethodCall { module: \"__perry_runtime\"";
    let mut from = 0;
    while let Some(at) = debug[from..].find(marker) {
        let after = from + at + marker.len();
        let method = debug[after..]
            .find("method: \"")
            .map(|m| after + m + "method: \"".len())
            .and_then(|m| debug[m..].find('"').map(|e| &debug[m..m + e]));
        match method {
            Some(name) if name.starts_with("iterator") => {}
            other => return Some(format!("runtime method {other:?}")),
        }
        from = after;
    }
    let mut unknown: BTreeSet<&str> = BTreeSet::new();
    let rb = rest.as_bytes();
    let mut k = 0;
    while k < rb.len() {
        let c = rb[k];
        if c.is_ascii_uppercase()
            && (k == 0 || !(rb[k - 1].is_ascii_alphanumeric() || rb[k - 1] == b'_'))
        {
            let start = k;
            while k < rb.len() && (rb[k].is_ascii_alphanumeric() || rb[k] == b'_') {
                k += 1;
            }
            let name = &rest[start..k];
            if !is_sync_name(name) {
                unknown.insert(name);
            }
        } else {
            k += 1;
        }
    }
    if !unknown.is_empty() {
        return Some(format!(
            "constructs not proven synchronous: {}",
            unknown.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_sync_constructs_pass() {
        let hir = r#"Module { init: [Expr(Call { callee: PropertyGet { object: GlobalGet(0), property: "log", byte_offset: 1 }, args: [ArrayMap { array: LocalGet(0), callback: Closure { func_id: 0, is_async: false } }] })] }"#;
        assert_eq!(why_module_not_synchronous(hir), None);
    }

    #[test]
    fn async_constructs_keep_the_loop() {
        for hir in [
            r#"Module { init: [Expr(Await(LocalGet(0)))] }"#,
            r#"Module { init: [Expr(Closure { func_id: 0, is_async: true })] }"#,
            r#"Module { init: [Expr(QueueMicrotask(LocalGet(0)))] }"#,
            r#"Module { init: [Expr(ProcessNextTick { callback: LocalGet(0) })] }"#,
            r#"Module { init: [Expr(PropertyGet { object: GlobalGet(0), property: "setTimeout", byte_offset: 0 })] }"#,
            r#"Module { init: [Expr(PropertyGet { object: GlobalGet(0), property: "Promise", byte_offset: 0 })] }"#,
            r#"Module { init: [Expr(PropertyGet { object: LocalGet(0), property: "constructor", byte_offset: 0 })] }"#,
            r#"Module { init: [Expr(Call { callee: ExternFuncRef { name: "js_set_timeout" }, args: [] })] }"#,
            r#"Module { init: [Expr(NativeMethodCall { module: "fs", class_name: None, object: None, method: "readFile", args: [] })] }"#,
            r#"Module { init: [Expr(ArrayFromAsync(LocalGet(0)))] }"#,
            r#"Module { init: [Expr(SomeFutureVariant(LocalGet(0)))] }"#,
        ] {
            assert!(why_module_not_synchronous(hir).is_some(), "{hir}");
        }
    }

    #[test]
    fn a_class_constructor_name_is_structure() {
        let hir = r#"Module { classes: [Class { constructor: Some(Function { id: 3, name: "constructor", is_async: false }) }] }"#;
        assert_eq!(why_module_not_synchronous(hir), None);
        let read = r#"Module { init: [Expr(PropertyGet { object: LocalGet(0), property: "constructor", byte_offset: 0 })] }"#;
        assert!(why_module_not_synchronous(read).is_some());
    }

    #[test]
    fn string_data_is_not_a_construct() {
        let hir = r#"Module { init: [Expr(String("Promise Await SetTimeout"))] }"#;
        assert_eq!(why_module_not_synchronous(hir), None);
    }
}
