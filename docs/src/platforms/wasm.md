# WebAssembly / Web

Perry compiles TypeScript apps to **WebAssembly** for the browser using `--target wasm` or its alias `--target web`. Both flags route through the same backend (`perry-codegen-wasm`) and produce the same output: a self-contained HTML file with embedded WASM bytecode and a thin JavaScript bridge for DOM widgets and host APIs.

There used to be a separate JavaScript-emitting `--target web` (`perry-codegen-js`); it was consolidated into the WASM target so browser apps get near-native performance, FFI imports, and Web Worker threading "for free".

## Building

```bash
# Self-contained HTML (default)
perry app.ts -o app --target web
open app.html

# Same thing
perry app.ts -o app --target wasm

# Raw .wasm binary (no HTML wrapper)
perry app.ts -o app.wasm --target wasm
```

The default output is a single `.html` file containing a base64-embedded WASM binary, the `wasm_runtime.js` bridge, and a `bootPerryWasm()` call that instantiates the module. Open it directly in any modern browser — no build step, no server required for simple apps.

> **Note**: Apps that use `fetch()` or other web platform APIs that depend on a real origin must be served over HTTP (file:// URLs run into CORS / "Failed to fetch" errors). Any local static server works:
> ```bash
> python3 -m http.server 8765
> open http://localhost:8765/app.html
> ```

## How It Works

The `perry-codegen-wasm` crate compiles HIR directly to WASM bytecode using `wasm-encoder`. The output WASM:

- Imports ~280 host functions under the `rt` namespace (string ops, math, console, JSON, classes, closures, promises, fetch, etc.)
- Imports user-declared FFI functions under the `ffi` namespace
- Exports `_start`, `memory`, `__indirect_function_table`, and every user function as `__wasm_func_<idx>` (so async function bodies compiled to JS can call back into WASM)

The NaN-boxing scheme matches the native `perry-runtime` — f64 values with STRING_TAG/POINTER_TAG/INT32_TAG — so the same value representation is used across native and WASM targets. The JS bridge wraps every host import with bit-level reinterpretation so f64 NaN-boxed values pass through the BigInt-based JS↔WASM i64 boundary intact (BigInt(NaN) would otherwise throw).

## Supported Features

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

## Module-Level Constants

Top-level `const`/`let` declarations are promoted to dedicated WASM globals so functions in the same module can read them, and so two modules' identical `LocalId`s don't collide:

```typescript
{{#include ../../examples/platforms/wasm_snippets.ts:module-const}}
```

Both `CHIRP_URL` and `API_KEY` become WASM globals indexed by `(module_idx, LocalId)`. Reading them from `trackEvent` emits a `global.get` instead of trying to look up a function-local that doesn't exist.

## JavaScript Runtime Bridge

The bridge (`wasm_runtime.js`) is embedded in the HTML and provides ~280 imports across:

- **NaN-boxing helpers**: `f64ToU64` / `u64ToF64` / `nanboxString` / `nanboxPointer` / `toJsValue` / `fromJsValue`
- **String table**: dynamic JS string array indexed by string ID
- **Handle store**: maps integer handle IDs to JS objects, arrays, closures, promises, DOM elements
- **Core ops**: console, math, JSON, JSON.parse/stringify, Date, RegExp, URL, Map, Set, Buffer, fetch
- **Closure dispatch**: indirect function table + capture array, with `closure_call_0/1/2/3/spread`
- **Class dispatch**: `class_new`, `class_call_method`, `class_get_field`, `class_set_field`, parent table for inheritance
- **DOM widgets**: 168+ `perry_ui_*` functions covering every widget in `perry/ui`
- **Async functions**: compiled to JS function bodies and merged into the import object as `__async_<name>`

All host imports are wrapped via `wrapImportsForI64()` so they automatically reinterpret BigInt args (from WASM i64 params) into f64 internally and reinterpret Number returns back into BigInt. Without this wrapping, every NaN-valued f64 return would crash with "Cannot convert NaN to a BigInt".

## Web Worker Threading

`perry/thread` works in the browser via a Web Worker pool:

```typescript
{{#include ../../examples/platforms/wasm_snippets.ts:web-worker-thread}}
```

Each worker instantiates its own WASM module with the same bytecode and bridge. Values cross between the main thread and workers via structured-clone serialization. See [Threading](../threading/overview.md).

## Limitations

- **No file system access** beyond the File System Access API (`window.showDirectoryPicker()`)
- **No raw TCP/UDP sockets** — only `fetch()` and `WebSocket`
- **No subprocess spawning** — `child_process.exec` etc. are no-ops
- **No native databases** — SQLite, Postgres, MySQL drivers don't compile to web
- **CORS** applies to all `fetch()` calls — third-party APIs must allow your origin
- **localStorage**, not real keychain — fine for preferences, not for secrets
- Source-mapped stack traces are JS-only; WASM stack frames show `wasm-function[N]`

## Minification

Use `--minify` to minify the embedded JS runtime bridge in the HTML output. The Rust-native JS minifier strips comments, collapses whitespace, and mangles internal identifiers, compressing the runtime from ~3,400 lines to ~180.

```bash
perry app.ts -o app --target web --minify
```

## Example: Counter App

```typescript
{{#include ../../examples/platforms/ui/counter_app.ts:counter}}
```

```bash
perry counter.ts -o counter --target web
open counter.html
```

## Example: Real-World App (Mango MongoDB GUI)

The [Mango](https://github.com/MangoQuery/app) MongoDB GUI — 50 modules, 998 functions, classes, async functions, fetch with custom headers, the Hone code editor — compiles to a single 4 MB HTML file via `--target web` and renders its full UI (welcome screen, query view, edit view) in the browser. SQLite-backed connection storage gracefully degrades to an in-memory transient store on web; the rest of the app works the same as the native version.

## Next Steps

- [Platform Overview](overview.md) — All platforms
- [UI Overview](../ui/overview.md) — UI system
- [Threading](../threading/overview.md) — Web Worker threading
