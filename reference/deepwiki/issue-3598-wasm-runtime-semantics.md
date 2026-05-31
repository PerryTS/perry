# DeepWiki Response

Repository: `PerryTS/perry`
Question: For PerryTS/perry, explain the existing WebAssembly runtime architecture and key files for implementing Node-compatible WebAssembly.compile, WebAssembly.instantiate, WebAssembly.Module, Instance, Memory, Table, Global, and WebAssembly.Module.exports/imports/customSections semantics. Focus on wasm-host support, crates/perry-codegen-wasm, crates/perry-runtime wasm files, HIR lowering/codegen paths, current limitations, and likely tests.
Search: https://deepwiki.com/search/for-perrytsperry-explain-the-e_bc465cad-9aa7-4150-beb0-01575d4beaf8

**Question**: For PerryTS/perry, explain the existing WebAssembly runtime architecture and key files for implementing Node-compatible WebAssembly.compile, WebAssembly.instantiate, WebAssembly.Module, Instance, Memory, Table, Global, and WebAssembly.Module.exports/imports/customSections semantics. Focus on wasm-host support, crates/perry-codegen-wasm, crates/perry-runtime wasm files, HIR lowering/codegen paths, current limitations, and likely tests.

Perry provides WebAssembly (WASM) runtime support primarily through the `perry-codegen-wasm` crate, which compiles High-level Intermediate Representation (HIR) to WASM bytecode, and the `perry-runtime` crate, which includes shims for WASM host functions [1](#ref-q1-1)  [2](#ref-q1-2) . This architecture enables Node-compatible `WebAssembly.compile`, `WebAssembly.instantiate`, and related semantics, with a focus on browser environments when targeting `web` or `wasm` [3](#ref-q1-3) .

## WebAssembly Runtime Architecture

The core of Perry's WebAssembly support involves several components:

### `perry-codegen-wasm`
This crate is responsible for translating Perry's HIR into WASM bytecode [4](#ref-q1-4) . It uses `wasm-encoder` to generate the WASM binary [5](#ref-q1-5) . The output can be a raw `.wasm` binary or a self-contained HTML file with the WASM embedded as base64 and a JavaScript runtime bridge [6](#ref-q1-6)  [7](#ref-q1-7) .

The `WasmModuleEmitter` in `crates/perry-codegen-wasm/src/emit/module_emitter.rs` orchestrates the compilation process [8](#ref-q1-8) . It handles the generation of various WASM sections, including imports, exports, functions, memory, and tables [9](#ref-q1-9) .

### `wasm_runtime.js`
This JavaScript bridge, embedded in the generated HTML, provides the host environment for the WASM module [10](#ref-q1-10)  [11](#ref-q1-11) . It handles:
*   **NaN-boxing helpers**: Converts between JavaScript values and the 64-bit NaN-boxed representation used by Perry's runtime [12](#ref-q1-12)  [13](#ref-q1-13) .
*   **Host function imports**: Provides implementations for standard library functions (e.g., `console`, `JSON`, `fetch`) and UI widgets, which are imported by the WASM module under the `rt` namespace [14](#ref-q1-14)  [15](#ref-q1-15) .
*   **FFI support**: Allows JavaScript functions to be imported into the WASM module under the `ffi` namespace [16](#ref-q1-16) .

### `perry-runtime` and `wasm-host` feature
The `perry-runtime` crate contains runtime shims for WebAssembly host functions [2](#ref-q1-2) . When the `wasm-host` Cargo feature is enabled, `webassembly.rs` is compiled into `libperry_runtime.a`, which declares C ABI functions like `perry_wasm_host_*` [17](#ref-q1-17)  [2](#ref-q1-2) . These are resolved by linking against the separate `perry-wasm-host` crate [17](#ref-q1-17) . This feature is automatically enabled if the compiled code references `WebAssembly.*` [18](#ref-q1-18)  or if the `--enable-wasm-runtime` flag is passed during compilation [19](#ref-q1-19) .

## Node-compatible WebAssembly Semantics

Perry aims to provide Node-compatible WebAssembly APIs, specifically:

*   `WebAssembly.compile`: While not explicitly detailed as a direct API, the compilation process from HIR to WASM bytecode by `perry-codegen-wasm` fulfills the role of compiling a module [4](#ref-q1-4) . In the `wasm_runtime.js` for Web Workers, `WebAssembly.compile(msg.binary)` is used to compile the raw WASM bytes [20](#ref-q1-20) .
*   `WebAssembly.instantiate`: This is directly supported. The `wasm_runtime.js` calls `WebAssembly.instantiate(wasmBytes, imports)` to create a WASM instance [21](#ref-q1-21) . The `perry-hir` lowering also recognizes `WebAssembly.instantiate(bytes)` and translates it into an `Expr::WebAssemblyInstantiate` HIR node [22](#ref-q1-22) .
*   `WebAssembly.Module`: The concept of a WASM module is central to `perry-codegen-wasm`, which generates the module's bytecode [4](#ref-q1-4) .
*   `WebAssembly.Instance`: Represented by the `instance` object returned by `WebAssembly.instantiate` in `wasm_runtime.js` [23](#ref-q1-23) .
*   `WebAssembly.Memory`, `WebAssembly.Table`, `WebAssembly.Global`: These are fundamental WASM concepts handled by `wasm-encoder` during bytecode generation. The `WasmModuleEmitter` explicitly adds `MemorySection`, `TableSection`, and `GlobalSection` [24](#ref-q1-24) . The `memory` and `__indirect_function_table` (which serves as the Table) are exported from the WASM module [25](#ref-q1-25) . Top-level `const`/`let` declarations in TypeScript are promoted to WASM globals [26](#ref-q1-26)  and exported as `__wasm_global_<idx>` [27](#ref-q1-27) .
*   `WebAssembly.Module.exports/imports/customSections`:
    *   **Exports**: User functions are exported as `__wasm_func_<idx>`, and module-level globals are exported as `__wasm_global_<idx>` [28](#ref-q1-28)  [27](#ref-q1-27) . The `_start`, `memory`, and `__indirect_function_table` are also exported [29](#ref-q1-29) .
    *   **Imports**: The WASM module imports host functions under the `rt` namespace and user-declared FFI functions under the `ffi` namespace [14](#ref-q1-14) . The `wasm_runtime.js` builds the `imports` object for `WebAssembly.instantiate` [30](#ref-q1-30) .
    *   **Custom Sections**: Not explicitly mentioned in the provided context, but `wasm-encoder` supports custom sections.

## HIR Lowering and Codegen Paths

The compilation pipeline involves:
1.  **Parsing**: TypeScript code is parsed into an Abstract Syntax Tree (AST) using SWC [31](#ref-q1-31) .
2.  **Lowering**: The AST is lowered into Perry's High-level Intermediate Representation (HIR) by `perry-hir` [32](#ref-q1-32) . During this phase, calls to `WebAssembly.validate`, `WebAssembly.instantiate`, and `WebAssembly.callExport` are recognized and converted into specific HIR expressions: `Expr::WebAssemblyValidate`, `Expr::WebAssemblyInstantiate`, and `Expr::WebAssemblyCallExport` respectively [33](#ref-q1-33)  [34](#ref-q1-34) .
3.  **Codegen**: For `--target wasm` or `--target web`, the HIR is passed to `perry-codegen-wasm` [35](#ref-q1-35) . This crate then emits the WASM bytecode. The `Expr::WebAssemblyValidate`, `Expr::WebAssemblyInstantiate`, and `Expr::WebAssemblyCallExport` HIR nodes are translated into calls to `js_webassembly_validate`, `js_webassembly_instantiate`, and `js_webassembly_call_export_N` (where N is the number of arguments) respectively [36](#ref-q1-36) . These `js_webassembly_*` functions are declared in `perry-codegen` [37](#ref-q1-37)  and implemented as shims in `perry-runtime` when the `wasm-host` feature is enabled [2](#ref-q1-2) .

## Current Limitations

The WebAssembly target has several limitations, primarily due to the browser sandbox environment [38](#ref-q1-38) :
*   **No direct file system access**: Beyond the File System Access API [39](#ref-q1-39) .
*   **No raw TCP/UDP sockets**: Only `fetch()` and `WebSocket` are available [40](#ref-q1-40) .
*   **No subprocess spawning** [41](#ref-q1-41) .
*   **No native databases** [42](#ref-q1-42) .
*   **CORS applies to `fetch()` calls** [43](#ref-q1-43) .
*   **`localStorage` instead of real keychain** [44](#ref-q1-44) .
*   **WASM stack frames show `wasm-function[N]`** in source-mapped stack traces [45](#ref-q1-45) .

## Likely Tests

Tests for WebAssembly functionality would likely involve:
*   **Compilation tests**: Verifying that TypeScript code using `WebAssembly.*` APIs compiles successfully to WASM and HTML [46](#ref-q1-46) .
*   **Runtime tests**: Executing the generated WASM in a browser environment to ensure `WebAssembly.instantiate`, `WebAssembly.validate`, and `WebAssembly.callExport` behave as expected [47](#ref-q1-47) .
*   **FFI tests**: Confirming that declared FFI functions are correctly imported and can be provided by the host JavaScript [48](#ref-q1-48) .
*   **Feature tests**: Testing specific supported features like async/await, threading via Web Workers, and UI widget rendering [49](#ref-q1-49) .
*   **Error handling**: Testing scenarios where `WebAssembly.*` is used without the `wasm-host` library, which should result in a compilation error [50](#ref-q1-50) .
*   **Obfuscation scanner tests**: Ensuring that the `WebAssembly.instantiate` calls are not incorrectly flagged as execution sinks [51](#ref-q1-51) .

## Notes

The `perry-codegen-js` crate is mentioned as a legacy JavaScript code generator, but its functionality for `--target web` has been consolidated into `perry-codegen-wasm` [52](#ref-q1-52) . It is still present for the JS minifier <cite repo="PerryTS/perry" path="crates/perry-codegen-wasm/src/lib.rs" start="28"

Wiki pages you might want to explore:
- [Project Configuration (perry.toml & package.json) (PerryTS/perry)](/wiki/PerryTS/perry#1.2)
- [Platform Targets Reference (PerryTS/perry)](/wiki/PerryTS/perry#9)

View this search on DeepWiki: https://deepwiki.com/search/for-perrytsperry-explain-the-e_bc465cad-9aa7-4150-beb0-01575d4beaf8

## References

<a id="ref-q1-1"></a>
### [1] `crates/perry-codegen-wasm/src/lib.rs:1-7`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/lib.rs#L1-L7)

```rust
//! WebAssembly code generation backend for Perry
//!
//! Compiles HIR modules to WebAssembly binary format for `--target wasm`.
//! Produces a self-contained HTML file with embedded WASM (base64) and JS runtime bridge.
//!
//! All JSValues use NaN-boxing (f64) consistent with perry-runtime.
//! Runtime operations (strings, console, objects) are imported from JavaScript.
```

<a id="ref-q1-2"></a>
### [2] `crates/perry-runtime/src/lib.rs:66-78`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/lib.rs#L66-L78)

```rust
/// WebAssembly host shims (issue #76). Forward-declares the
/// `perry_wasm_host_*` C ABI; the wasmi-backed implementation lives in
/// the separate `perry-wasm-host` crate and is linked in only when the
/// user passes `--enable-wasm-runtime`.
///
/// Gated behind the `wasm-host` Cargo feature so non-wasm programs don't
/// pull `js_webassembly_*` into libperry_runtime.a — those shims hold
/// undefined references to `perry_wasm_host_*` which would fail to link
/// without libperry_wasm_host.a on the line. The auto-optimize path
/// (crates/perry/src/commands/compile/optimized_libs.rs) flips this
/// feature on when `ctx.needs_wasm_runtime` is true.
#[cfg(feature = "wasm-host")]
pub mod webassembly;
```

<a id="ref-q1-3"></a>
### [3] `docs/src/platforms/wasm.md:3-5`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L3-L5)

```markdown
Perry compiles TypeScript apps to **WebAssembly** for the browser using `--target wasm` or its alias `--target web`. Both flags route through the same backend (`perry-codegen-wasm`) and produce the same output: a self-contained HTML file with embedded WASM bytecode and a thin JavaScript bridge for DOM widgets and host APIs.

There used to be a separate JavaScript-emitting `--target web` (`perry-codegen-js`); it was consolidated into the WASM target so browser apps get near-native performance, FFI imports, and Web Worker threading "for free".
```

<a id="ref-q1-4"></a>
### [4] `crates/perry-codegen-wasm/src/lib.rs:1-3`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/lib.rs#L1-L3)

```rust
//! WebAssembly code generation backend for Perry
//!
//! Compiles HIR modules to WebAssembly binary format for `--target wasm`.
```

<a id="ref-q1-5"></a>
### [5] `docs/src/platforms/wasm.md:31-32`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L31-L32)

```markdown
The `perry-codegen-wasm` crate compiles HIR directly to WASM bytecode using `wasm-encoder`. The output WASM:
```

<a id="ref-q1-6"></a>
### [6] `crates/perry-codegen-wasm/src/lib.rs:3-5`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/lib.rs#L3-L5)

```rust
//! Compiles HIR modules to WebAssembly binary format for `--target wasm`.
//! Produces a self-contained HTML file with embedded WASM (base64) and JS runtime bridge.
//!
```

<a id="ref-q1-7"></a>
### [7] `docs/src/platforms/wasm.md:17-22`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L17-L22)

```markdown
# Raw .wasm binary (no HTML wrapper)
perry app.ts -o app.wasm --target wasm
```

The default output is a single `.html` file containing a base64-embedded WASM binary, the `wasm_runtime.js` bridge, and a `bootPerryWasm()` call that instantiates the module. Open it directly in any modern browser — no build step, no server required for simple apps.
```

<a id="ref-q1-8"></a>
### [8] `crates/perry-codegen-wasm/src/emit/mod.rs:17-18`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/emit/mod.rs#L17-L18)

```rust
//! - `module_emitter` — `WasmModuleEmitter` struct + `new` / `intern_string` / `get_type_idx`
//! - `compile` — the giant `WasmModuleEmitter::compile` orchestration method
```

<a id="ref-q1-9"></a>
### [9] `crates/perry-codegen-wasm/src/emit/mod.rs:45-49`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/emit/mod.rs#L45-L49)

```rust
use wasm_encoder::{
    CodeSection, DataSection, ElementSection, Elements, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, GlobalSection, GlobalType, Ieee64, ImportSection, Instruction,
    MemorySection, MemoryType, Module, RefType, TableSection, TableType, TypeSection, ValType,
};
```

<a id="ref-q1-10"></a>
### [10] `crates/perry-codegen-wasm/src/lib.rs:16`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/lib.rs#L16)

```rust
const WASM_RUNTIME_JS: &str = include_str!("wasm_runtime.js");
```

<a id="ref-q1-11"></a>
### [11] `docs/src/platforms/wasm.md:105-107`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L105-L107)

```markdown
## JavaScript Runtime Bridge

The bridge (`wasm_runtime.js`) is embedded in the HTML and provides ~280 imports across:
```

<a id="ref-q1-12"></a>
### [12] `crates/perry-codegen-wasm/src/wasm_runtime.js:2279-2298`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/wasm_runtime.js#L2279-L2298)

```javascript
function __jsValueToBits(v) {
  if (v === undefined) return TAG_UNDEFINED;
  if (v === null) return TAG_NULL;
  if (v === true) return TAG_TRUE;
  if (v === false) return TAG_FALSE;
  if (typeof v === "number") { _f64[0] = v; return _u64[0]; }
  if (typeof v === "string") {
    const id = stringTable.length; stringTable.push(v);
    return (STRING_TAG << 48n) | BigInt(id);
  }
  if (Array.isArray(v)) {
    const id = allocHandle(v);
    return (POINTER_TAG << 48n) | BigInt(id);
  }
  if (typeof v === "object") {
    const id = allocHandle(v);
    return (POINTER_TAG << 48n) | BigInt(id);
  }
  _f64[0] = Number(v); return _u64[0];
}
```

<a id="ref-q1-13"></a>
### [13] `docs/src/platforms/wasm.md:34-37`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L34-L37)

```markdown
- Imports user-declared FFI functions under the `ffi` namespace
- Exports `_start`, `memory`, `__indirect_function_table`, and every user function as `__wasm_func_<idx>` (so async function bodies compiled to JS can call back into WASM)

The NaN-boxing scheme matches the native `perry-runtime` — f64 values with STRING_TAG/POINTER_TAG/INT32_TAG — so the same value representation is used across native and WASM targets. The JS bridge wraps every host import with bit-level reinterpretation so f64 NaN-boxed values pass through the BigInt-based JS↔WASM i64 boundary intact (BigInt(NaN) would otherwise throw).
```

<a id="ref-q1-14"></a>
### [14] `docs/src/platforms/wasm.md:32-34`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L32-L34)

```markdown

- Imports ~280 host functions under the `rt` namespace (string ops, math, console, JSON, classes, closures, promises, fetch, etc.)
- Imports user-declared FFI functions under the `ffi` namespace
```

<a id="ref-q1-15"></a>
### [15] `docs/src/platforms/wasm.md:107-117`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L107-L117)

```markdown
The bridge (`wasm_runtime.js`) is embedded in the HTML and provides ~280 imports across:

- **NaN-boxing helpers**: `f64ToU64` / `u64ToF64` / `nanboxString` / `nanboxPointer` / `toJsValue` / `fromJsValue`
- **String table**: dynamic JS string array indexed by string ID
- **Handle store**: maps integer handle IDs to JS objects, arrays, closures, promises, DOM elements
- **Core ops**: console, math, JSON, JSON.parse/stringify, Date, RegExp, URL, Map, Set, Buffer, fetch
- **Closure dispatch**: indirect function table + capture array, with `closure_call_0/1/2/3/spread`
- **Class dispatch**: `class_new`, `class_call_method`, `class_get_field`, `class_set_field`, parent table for inheritance
- **DOM widgets**: 168+ `perry_ui_*` functions covering every widget in `perry/ui`
- **Async functions**: compiled to JS function bodies and merged into the import object as `__async_<name>`
```

<a id="ref-q1-16"></a>
### [16] `docs/src/platforms/wasm.md:75-77`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L75-L77)

```markdown
## FFI Support

The WASM target supports external FFI functions declared with `declare function`. They become WASM imports under the `"ffi"` namespace:
```

<a id="ref-q1-17"></a>
### [17] `crates/perry-runtime/Cargo.toml:37-38`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/Cargo.toml#L37-L38)

```
# programs don't pay an unresolvable-symbol penalty at link time.
wasm-host = []
```

<a id="ref-q1-18"></a>
### [18] `crates/perry/src/commands/compile/collect_modules.rs:1235-1242`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry/src/commands/compile/collect_modules.rs#L1235-L1242)

```rust
    // Issue #76 — auto-link the wasmi host runtime when any module
    // references `WebAssembly.*`. Without this the user has to remember
    // `--enable-wasm-runtime`; with it the flag is only needed when they
    // want to override the auto-detection (e.g. force-link for plugins
    // they'll dlopen later).
    if hir_module.uses_webassembly {
        ctx.needs_wasm_runtime = true;
    }
```

<a id="ref-q1-19"></a>
### [19] `crates/perry/src/commands/compile/types.rs:74-78`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry/src/commands/compile/types.rs#L74-L78)

```rust
    /// Enable WebAssembly host runtime so the produced binary can load .wasm
    /// modules at runtime via `WebAssembly.instantiate(bytes)`. Engine: wasmi
    /// (pure-Rust interpreter). Adds ~1MB to the binary. Issue #76.
    #[arg(long)]
    pub enable_wasm_runtime: bool,
```

<a id="ref-q1-20"></a>
### [20] `crates/perry-codegen-wasm/src/wasm_runtime.js:2325`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/wasm_runtime.js#L2325)

```javascript
      const module = await WebAssembly.compile(msg.binary);
```

<a id="ref-q1-21"></a>
### [21] `crates/perry-codegen-wasm/src/wasm_runtime.js:4068`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/wasm_runtime.js#L4068)

```javascript
  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
```

<a id="ref-q1-22"></a>
### [22] `crates/perry-hir/src/lower/expr_call/module_static.rs:431-436`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-hir/src/lower/expr_call/module_static.rs#L431-L436)

```rust
                        "instantiate" => {
                            if !args.is_empty() {
                                ctx.uses_webassembly = true;
                                return Ok(Ok(Expr::WebAssemblyInstantiate(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
```

<a id="ref-q1-23"></a>
### [23] `crates/perry-codegen-wasm/src/wasm_runtime.js:4068-4069`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/wasm_runtime.js#L4068-L4069)

```javascript
  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
  wasmInstance = instance;
```

<a id="ref-q1-24"></a>
### [24] `crates/perry-codegen-wasm/src/emit/mod.rs:48-49`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/emit/mod.rs#L48-L49)

```rust
    MemorySection, MemoryType, Module, RefType, TableSection, TableType, TypeSection, ValType,
};
```

<a id="ref-q1-25"></a>
### [25] `crates/perry-codegen-wasm/src/emit/compile.rs:1166-1167`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/emit/compile.rs#L1166-L1167)

```rust
        export_section.export("memory", ExportKind::Memory, 0);
        export_section.export("__indirect_function_table", ExportKind::Table, 0);
```

<a id="ref-q1-26"></a>
### [26] `docs/src/platforms/wasm.md:96-97`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L96-L97)

```markdown

Top-level `const`/`let` declarations are promoted to dedicated WASM globals so functions in the same module can read them, and so two modules' identical `LocalId`s don't collide:
```

<a id="ref-q1-27"></a>
### [27] `crates/perry-codegen-wasm/src/emit/compile.rs:1188-1189`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/emit/compile.rs#L1188-L1189)

```rust
            for gidx in exported_globals {
                export_section.export(&format!("__wasm_global_{}", gidx), ExportKind::Global, gidx);
```

<a id="ref-q1-28"></a>
### [28] `crates/perry-codegen-wasm/src/emit/compile.rs:1168-1171`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/emit/compile.rs#L1168-L1171)

```rust
        // Export all user functions so async JS code can call them by index.
        for idx in self.num_imports..start_idx {
            export_section.export(&format!("__wasm_func_{}", idx), ExportKind::Func, idx);
        }
```

<a id="ref-q1-29"></a>
### [29] `crates/perry-codegen-wasm/src/emit/compile.rs:1165-1167`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/emit/compile.rs#L1165-L1167)

```rust
        export_section.export("_start", ExportKind::Func, start_idx);
        export_section.export("memory", ExportKind::Memory, 0);
        export_section.export("__indirect_function_table", ExportKind::Table, 0);
```

<a id="ref-q1-30"></a>
### [30] `crates/perry-codegen-wasm/src/wasm_runtime.js:4067`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/wasm_runtime.js#L4067)

```javascript
  const imports = wrapImportsForI64(buildImports());
```

<a id="ref-q1-31"></a>
### [31] `docs/src/contributing/architecture.md:9-10`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/contributing/architecture.md#L9-L10)

```markdown
    ↓ Parse (SWC)
    ↓ AST
```

<a id="ref-q1-32"></a>
### [32] `docs/src/contributing/architecture.md:11-12`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/contributing/architecture.md#L11-L12)

```markdown
    ↓ Lower (perry-hir)
    ↓ HIR (High-level IR)
```

<a id="ref-q1-33"></a>
### [33] `crates/perry-hir/src/lower/expr_call/module_static.rs:419-447`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-hir/src/lower/expr_call/module_static.rs#L419-L447)

```rust
            if obj_ident.sym.as_ref() == "WebAssembly" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "validate" => {
                            if !args.is_empty() {
                                ctx.uses_webassembly = true;
                                return Ok(Ok(Expr::WebAssemblyValidate(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            }
                        }
                        "instantiate" => {
                            if !args.is_empty() {
                                ctx.uses_webassembly = true;
                                return Ok(Ok(Expr::WebAssemblyInstantiate(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            }
                        }
                        "callExport" => {
                            if args.len() >= 2 {
                                ctx.uses_webassembly = true;
                                let mut it = args.into_iter();
                                let instance = it.next().unwrap();
                                let name = it.next().unwrap();
                                let rest: Vec<Expr> = it.collect();
                                return Ok(Ok(Expr::WebAssemblyCallExport {
                                    instance: Box::new(instance),
```

<a id="ref-q1-34"></a>
### [34] `crates/perry-hir/src/ir/expr.rs:708-718`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-hir/src/ir/expr.rs#L708-L718)

```rust
    WebAssemblyValidate(Box<Expr>),
    /// `WebAssembly.instantiate(bytes)` -> opaque instance handle (Perry
    /// MVP shape — sync, no Promise, no `{module, instance}` pair).
    WebAssemblyInstantiate(Box<Expr>),
    /// `WebAssembly.callExport(instance, name, ...args)` — Perry-specific
    /// helper for invoking numeric exports (see issue #76 PoC scope).
    WebAssemblyCallExport {
        instance: Box<Expr>,
        name: Box<Expr>,
        args: Vec<Expr>,
    },
```

<a id="ref-q1-35"></a>
### [35] `docs/src/contributing/architecture.md:31`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/contributing/architecture.md#L31)

```markdown
| `perry-codegen-wasm` | WebAssembly code generation for `--target web` / `--target wasm` (HIR → WASM bytecode + JS bridge) |
```

<a id="ref-q1-36"></a>
### [36] `crates/perry-codegen/src/expr/math_simple.rs:251-322`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen/src/expr/math_simple.rs#L251-L322)

```rust
        Expr::WebAssemblyValidate(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_validate", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyInstantiate(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_instantiate", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyCallExport {
            instance,
            name,
            args,
        } => {
            let inst = lower_expr(ctx, instance)?;
            let name_v = lower_expr(ctx, name)?;
            let lowered_args: Vec<String> = args
                .iter()
                .map(|a| lower_expr(ctx, a))
                .collect::<Result<Vec<_>>>()?;
            let blk = ctx.block();
            match lowered_args.len() {
                0 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_0",
                    &[(DOUBLE, &inst), (DOUBLE, &name_v)],
                )),
                1 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_1",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                    ],
                )),
                2 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_2",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                        (DOUBLE, &lowered_args[1]),
                    ],
                )),
                3 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_3",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                        (DOUBLE, &lowered_args[1]),
                        (DOUBLE, &lowered_args[2]),
                    ],
                )),
                _ => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_4",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                        (DOUBLE, &lowered_args[1]),
                        (DOUBLE, &lowered_args[2]),
                        (DOUBLE, &lowered_args[3]),
                    ],
                )),
```

<a id="ref-q1-37"></a>
### [37] `crates/perry-codegen/src/runtime_decls/strings.rs:457-479`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen/src/runtime_decls/strings.rs#L457-L479)

```rust
    module.declare_function("js_webassembly_validate", DOUBLE, &[DOUBLE]);
    module.declare_function("js_webassembly_instantiate", DOUBLE, &[DOUBLE]);
    module.declare_function("js_webassembly_call_export_0", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_webassembly_call_export_1",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_webassembly_call_export_2",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_webassembly_call_export_3",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_webassembly_call_export_4",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
```

<a id="ref-q1-38"></a>
### [38] `docs/src/platforms/wasm.md:130-131`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L130-L131)

```markdown
## Limitations
```

<a id="ref-q1-39"></a>
### [39] `docs/src/platforms/wasm.md:131-132`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L131-L132)

```markdown

- **No file system access** beyond the File System Access API (`window.showDirectoryPicker()`)
```

<a id="ref-q1-40"></a>
### [40] `docs/src/platforms/wasm.md:133`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L133)

```markdown
- **No raw TCP/UDP sockets** — only `fetch()` and `WebSocket`
```

<a id="ref-q1-41"></a>
### [41] `docs/src/platforms/wasm.md:134`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L134)

```markdown
- **No subprocess spawning** — `child_process.exec` etc. are no-ops
```

<a id="ref-q1-42"></a>
### [42] `docs/src/platforms/wasm.md:135`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L135)

```markdown
- **No native databases** — SQLite, Postgres, MySQL drivers don't compile to web
```

<a id="ref-q1-43"></a>
### [43] `docs/src/platforms/wasm.md:136`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L136)

```markdown
- **CORS** applies to all `fetch()` calls — third-party APIs must allow your origin
```

<a id="ref-q1-44"></a>
### [44] `docs/src/platforms/wasm.md:137`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L137)

```markdown
- **localStorage**, not real keychain — fine for preferences, not for secrets
```

<a id="ref-q1-45"></a>
### [45] `docs/src/platforms/wasm.md:138`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L138)

```markdown
- Source-mapped stack traces are JS-only; WASM stack frames show `wasm-function[N]`
```

<a id="ref-q1-46"></a>
### [46] `docs/src/platforms/wasm.md:9-19`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L9-L19)

```markdown
```bash
# Self-contained HTML (default)
perry app.ts -o app --target web
open app.html

# Same thing
perry app.ts -o app --target wasm

# Raw .wasm binary (no HTML wrapper)
perry app.ts -o app.wasm --target wasm
```
```

<a id="ref-q1-47"></a>
### [47] `crates/perry-codegen-wasm/src/wasm_runtime.js:4058-4077`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen-wasm/src/wasm_runtime.js#L4058-L4077)

```javascript
async function bootPerryWasm(wasmBase64, ffiImports) {
  if (ffiImports) {
    if (typeof __ffiImports === 'undefined') {
      globalThis.__ffiImports = ffiImports;
    } else {
      Object.assign(__ffiImports, ffiImports);
    }
  }
  const wasmBytes = Uint8Array.from(atob(wasmBase64), c => c.charCodeAt(0));
  const imports = wrapImportsForI64(buildImports());
  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
  wasmInstance = instance;
  wasmMemory = instance.exports.memory;
  // Call the entry point
  if (instance.exports._start) {
    instance.exports._start();
  } else if (instance.exports.main) {
    instance.exports.main();
  }
}
```

<a id="ref-q1-48"></a>
### [48] `docs/src/platforms/wasm.md:75-93`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L75-L93)

```markdown
## FFI Support

The WASM target supports external FFI functions declared with `declare function`. They become WASM imports under the `"ffi"` namespace:

```typescript
{{#include ../../examples/platforms/wasm_snippets.ts:ffi-declares}}
```

Provide them when instantiating:

```javascript
// Via __ffiImports global (set before boot)
globalThis.__ffiImports = { bloom_init_window: ..., bloom_draw_rect: ... };

// Or via bootPerryWasm second argument
await bootPerryWasm(wasmBase64, { bloom_init_window: ..., bloom_draw_rect: ... });
```

**Auto-stub for missing imports.** The `ffi` namespace is wrapped in a `Proxy` so any FFI function the host doesn't provide is auto-stubbed with a no-op that returns `TAG_UNDEFINED`. This means apps that use native libraries (e.g. Hone Editor's 56 `hone_editor_*` functions) can still instantiate and run in the browser even without the native bindings — the relevant features are simply no-ops.
```

<a id="ref-q1-49"></a>
### [49] `docs/src/platforms/wasm.md:40-74`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/platforms/wasm.md#L40-L74)

```markdown

- **Full TypeScript language**: classes (with constructors, methods, getters/setters, inheritance, fields), async/await, closures (with captures), generators, destructuring, template literals, generics, enums, try/catch/finally
- **Module system**: cross-module imports, top-level `const`/`let` (promoted to WASM globals), circular imports
- **Standard library**: String/Array/Object methods, Map/Set, JSON, Date, RegExp, Math, Error, URL/URLSearchParams, Buffer, Promise (with `.then`/`.catch`/`.allSettled`/`.race`/`.any`/`.all`)
- **Async**: `async`/`await` (compiled to JS Promises), `setTimeout`/`setInterval`, `fetch()` with full request options (method, headers, body)
- **Threading**: `perry/thread` `parallelMap`/`parallelFilter`/`spawn` via Web Worker pool with one WASM instance per worker (see [Threading](../threading/overview.md))
- **DOM-based UI**: every widget in `perry/ui` (`VStack`, `HStack`, `ZStack`, `Text`, `Button`, `TextField`, `Toggle`, `Slider`, `ScrollView`, `Picker`, `Image`, `Canvas`, `Form`, `Section`, `NavigationStack`, `Table`, `LazyVStack`, `TextArea`, etc.) maps to a DOM element with flexbox layout. State bindings (`bindText`/`bindSlider`/`bindToggle`/`bindForEach`/...) work via reactive subscribers.
- **System APIs**: `localStorage`-backed preferences/keychain, dark mode detection (`prefers-color-scheme`), Web Notifications, clipboard, file open/save dialogs, File System Access API, Web Audio capture
- **FFI**: `declare function` declarations become WASM imports under the `ffi` namespace
- **Compile-time i18n**: `perry/i18n` `t()` calls work the same as native targets

## UI Mapping

Perry widgets map to HTML elements:

| Perry Widget | HTML Element |
|-------------|-------------|
| `Text` | `<span>` |
| `Button` | `<button>` |
| `TextField` | `<input type="text">` |
| `SecureField` | `<input type="password">` |
| `Toggle` | `<input type="checkbox">` |
| `Slider` | `<input type="range">` |
| `Picker` | `<select>` |
| `ProgressView` | `<progress>` |
| `Image` / `ImageFile` | `<img>` |
| `VStack` | `<div>` (flexbox column) |
| `HStack` | `<div>` (flexbox row) |
| `ZStack` | `<div>` (position: relative + absolute children) |
| `ScrollView` | `<div>` (overflow: auto) |
| `Canvas` | `<canvas>` (2D context) |
| `Table` | `<table>` |
| `Divider` | `<hr>` |
| `Spacer` | `<div>` (flex: 1) |
```

<a id="ref-q1-50"></a>
### [50] `crates/perry/src/commands/compile.rs:4616-4620`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry/src/commands/compile.rs#L4616-L4620)

```rust
                if ctx.needs_wasm_runtime {
                    return Err(anyhow!(
                        "WebAssembly.* used but libperry_wasm_host.a not found. Build it with: cargo build --release -p perry-wasm-host"
                    ));
                }
```

<a id="ref-q1-51"></a>
### [51] `crates/perry/src/commands/install/scanner/obfuscation.rs:264-281`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry/src/commands/install/scanner/obfuscation.rs#L264-L281)

```rust
    fn does_not_flag_wasm_blob() {
        // cjs-module-lexer shape: embedded WASM bytes, instantiated via
        // WebAssembly.* APIs. No eval/Function.
        let td = TempDir::new().unwrap();
        let blob: String = "D".repeat(1100);
        let body = format!(
            "const bytes = Buffer.from(\"{blob}\", 'base64');\n\
             WebAssembly.instantiate(bytes).then(m => module.exports = m.instance.exports);"
        );
        let p = make_pkg(
            &td,
            json!({"name":"wasm-lib","version":"1","main":"index.js"}),
            &[("index.js", body.as_str())],
        );
        assert!(
            check(&p).is_empty(),
            "WebAssembly host call should not count as an execution sink"
        );
```

<a id="ref-q1-52"></a>
### [52] `docs/src/contributing/architecture.md:32`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/contributing/architecture.md#L32)

```markdown
| `perry-codegen-js` | Legacy JavaScript code generator (still present for the JS minifier; the JS-emit `--target web` path was consolidated into `perry-codegen-wasm`) |
```
