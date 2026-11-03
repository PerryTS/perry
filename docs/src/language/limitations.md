# Limitations

Perry compiles a practical subset of TypeScript. This page documents what's not supported or works differently from Node.js/tsc.

## No Runtime Type Validation

Declared TypeScript types are not enforced at runtime — Perry doesn't generate
type guards from annotations, so a parameter typed `string` will accept a number
without throwing.

```typescript
{{#include ../../examples/language/limitations.ts:erased-types}}
```

Annotations are mostly erased, with one exception: when `emitDecoratorMetadata`
applies, the `design:type` / `design:paramtypes` reflection metadata is derived
from the annotations on decorated members and survives to runtime (see
[Decorators](decorators.md)). Runtime type *discrimination* is available via
explicit `typeof` checks and `instanceof`.

## Dynamic code and module loading {#dynamic-code-and-module-loading}

Constant `eval` and `Function` bodies can compile ahead of time. Runtime-generated bodies reach Perry’s linked interpreter; they are not automatically deferred errors. Interpreter support does not imply full engine parity or the ability to load arbitrary JavaScript packages from disk.

Dynamic `import()` and `require()` can resolve supported builtins and modules included in the compiled graph. Installing a package after compilation does not add it to that graph. Unavailable code targets fail at runtime; resolvable literals and supported finite-specifier forms can be compiled into the executable.

### Native OpenCode plugins (#10105)

The first native OpenCode deliverable follows **option A** from
[#10105](https://github.com/PerryTS/perry/issues/10105), tracked in
[#10107](https://github.com/PerryTS/perry/issues/10107): runtime-installed npm
plugins, local JS/TS/TSX plugins (including TUI plugins), custom tools, and provider
SDKs absent from the compiled graph are unavailable. Installing a package after
compilation does not add its code to the executable. Bundled providers reached
through static imports remain supported.

Use the Bun distribution when those extensions are required. Including extensions
in a statically reachable import graph and recompiling is a build-time option;
there is currently no automatic plugin-pack install/recompile command. A whole
ESM/CJS module interpreter (option B) and plugin-pack compilation/loading (option C)
are deferred decisions, not capabilities enabled by the existing dynamic-eval
interpreter. Runtime data imports such as TOML are a separate feature (#10104).

OpenCode v1.18.30's `PluginLoader.load` catches import rejections, and its caller
reports a load failure without retrying that stage. The TUI plugin reporter
includes `error.message`, so it can explain the native limitation and continue
startup. Perry's regression test covers that minimized loader/report/startup
contract with `plugin: ["some-npm-plugin"]`, one report, and successful builtin and
bundled imports afterward. Full OpenTUI rendering remains part of the integration
acceptance in #10107.

Data files can be loaded at runtime using import attributes (#10104). The
specifier must be an absolute filesystem path or a `file://` URL, and the
result has a `default` export:

| Import attribute `type` | Default export |
|---|---|
| `"toml"` | Parsed TOML table, using the same parser as `Bun.TOML.parse` |
| `"json"` | Parsed JSON value |
| `"text"` | File contents as a string |
| `"file"` | Filesystem path as a string |

```typescript,no-test
import { pathToFileURL } from "node:url";
const { default: config } = await import(pathToFileURL(configPath).href, {
  with: { type: "toml" },
});
```

TOML and JSON parse failures reject with `SyntaxError`. Missing files reject
with an I/O error. These loaders do not load runtime code modules or resolve
relative paths, package names, or network URLs. Strict mode still rejects
runtime-computed specifiers at compile time as described below.

`--strict-eval` remains accepted but does not reject bodies sent to the interpreter. `--strict-dynamic-import` still rejects unresolved asynchronous import specifiers during module collection; synchronous `require()` takes its runtime resolution path. `PERRY_ALLOW_EVAL=1` disables that strict import check.

## Decorators

Perry parses decorator syntax, supports compile-time-only transforms
(see the bundled `@log` example), and has a reduced legacy TypeScript
compatibility path for class decorators, method decorators, constructor
parameter decorators, method parameter decorators, and property
decorators. That path emits `design:paramtypes` for decorated
classes/methods, `design:type` for decorated properties, and implements
`Reflect.defineMetadata`, `Reflect.getMetadata`,
`Reflect.getOwnMetadata`, `Reflect.hasMetadata`,
`Reflect.hasOwnMetadata`, `Reflect.getMetadataKeys`,
`Reflect.getOwnMetadataKeys`, `Reflect.deleteMetadata`, and
`@Reflect.metadata(...)`.

Accessor decorators, descriptor replacement, general
`Reflect.metadata(...)` calls outside decorator syntax, `Symbol`
metadata keys, and full Angular / NestJS / TypeORM runtime metadata flows
are not supported. See [Decorators](decorators.md) for details and a
worked migration recipe.

## No Runtime Metadata Reflection

Perry implements a small metadata subset for legacy decorators. General
runtime reflection is not supported:

<!-- intentionally-rejects: this snippet documents code Perry refuses to compile -->
```text
Reflect.getMetadata("design:type", target, key);
Reflect.getMetadataKeys(target, key);
// Not supported as a general helper call outside decorator syntax
Reflect.metadata("design:type", String)(target, key);
```

## JavaScript and CommonJS

Perry accepts `.js`, `.cjs`, `.mjs` and `.jsx` through the native pipeline. CommonJS `require()` is supported for compiled modules and supported native modules. It does not turn the executable into a general filesystem JavaScript loader.

## Prototype Manipulation

Dynamic prototype manipulation is supported for the common patterns:

```typescript,no-test
// Supported
MyClass.prototype.newMethod = function () {};   // prototype method assignment
Object.setPrototypeOf(obj, proto);              // incl. chains of any length
Object.setPrototypeOf(Derived, Base);           // transpiled __extends statics link
Object.getPrototypeOf(obj);                     // inspection
```

Prototype-method assignment routes through a dedicated registry
(`CLASS_PROTOTYPE_METHODS`) and `Object.setPrototypeOf` performs a real,
cycle-checked prototype mutation (a genuine cycle still throws `TypeError`).

Remaining known gaps:

- the assignment form `obj.__proto__ = x` on the dynamic write path stores a
  literal `"__proto__"` property instead of setting the prototype (use
  `Object.setPrototypeOf`);
- an object-literal `{ __proto__: [] }` sets the prototype without throwing,
  but the result is not yet visible to `instanceof` (`… instanceof Array`
  reports `false`).

## Weak references and finalization

WeakMap, WeakSet, WeakRef and FinalizationRegistry participate in weak processing. Weak targets are excluded from the strong trace, forwarded addresses are repaired, dead targets can be cleared and cleanup callbacks can be queued. Collection and callback timing are not deterministic. See [GC weak processing](../internals/garbage-collector.md#barriers-and-weak-references).

## Limited Proxy Trapping

Proxy support is not a full engine-level trap layer for every possible dynamic
object access. Prefer plain objects and explicit APIs unless a package only
needs Perry's supported Proxy surface.

## Threading Model

Perry supports real multi-threading via `parallelMap` and `spawn` from `perry/thread`. See [Multi-Threading](../threading/overview.md).

Threads do not share mutable state by default — closures passed to thread
primitives cannot capture mutable variables (enforced at compile time), and
values are deep-copied across thread boundaries. The exception is
`SharedArrayBuffer`: a SAB captured into a `spawn` / `parallelMap` closure now
**aliases the same physical bytes** across agents, and `Atomics`
(`add`/`load`/`store`/`compareExchange`/… plus a real blocking
`wait`/`notify`/`waitAsync`) operate on it for genuine cross-thread coordination.
Caveat: only the `SharedArrayBuffer` itself shares — a typed-array *view*
captured directly still deep-copies, so build the view per-agent from the shared
SAB.

## WebAssembly (#6558)

Perry ships **no WebAssembly engine** in the default build. The adopted
policy is that **WASM-dependent features degrade gracefully**. Closed issue
[#6558](https://github.com/PerryTS/perry/issues/6558) records that decision and
the per-module replacement policy; it is not an active promise to add a full
engine to the default binary.

The `WebAssembly` global is spec-shaped — every standard member exists with
the correct type and arity — and fails cleanly rather than crashing:

- `WebAssembly.compile` / `compileStreaming` / `instantiate` /
  `instantiateStreaming` return a Promise **rejected** with a
  `WebAssembly.CompileError` whose message points at #6558.
- `WebAssembly.validate(bytes)` returns `false` — the honest answer for
  "can I run this module here".
- `new WebAssembly.Module(...)` throws `CompileError` synchronously;
  `Instance` throws `LinkError`; `Table` / `Global` throw `RuntimeError`.
- `new WebAssembly.Memory({ initial })` genuinely works (a real zero-filled
  `ArrayBuffer` backs it, and `grow()` is supported), so feature-detection
  code that allocates a page succeeds.
- `CompileError` / `LinkError` / `RuntimeError` are real error constructors
  (`instanceof Error` and `instanceof WebAssembly.CompileError` both work).

In practice this means lazy wasm consumers — wasm-bindgen loaders like
`@silvia-odwyer/photon-node`, `@jsquash/webp` decoders, undici's llhttp
probe — hit their own catch/fallback paths and the app keeps running with
that feature degraded.

When the wasmi host is linked, `new WebAssembly.Module(bytes)` and the
synchronous `new WebAssembly.Instance(module, imports)` constructor execute
real modules in Perry's numeric function/import subset. This includes WASM
bytes exposed through the embedded `import ... with { type: "file" }` loader;
the JS-visible `memory.buffer` is synchronized before and after calls,
numeric multi-value results use the standard array shape, and exported
`externref` tables support `get`, `set`, and `grow`. Imported tables/memories,
globals, general reference-valued function signatures, and streaming remain
outside that subset.

Two spellings, two paths:

- **Namespace access** (`const WA = globalThis.WebAssembly; WA.compile(...)`,
  which is also what minified bundles evaluate through a namespace value)
  uses the graceful surface above and needs nothing at link time.
- **Literal static calls** (`WebAssembly.instantiate(bytes)` written against
  the global) lower to the opt-in wasmi host runtime (issue #76,
  `--enable-wasm-runtime`, auto-linked when usage is detected). These run
  real WebAssembly through the wasmi interpreter but require
  `libperry_wasm_host.a` to be built (`cargo build --release -p
  perry-wasm-host`); without it the build fails with that instruction.

## npm Package Compatibility

Not all npm packages work with Perry:

- **Natively supported**: ~50 popular packages (fastify, mysql2, redis, etc.) — these are compiled natively. See [Standard Library](../stdlib/overview.md).
- **`compilePackages`**: Pure TS/JS packages can be compiled natively via [configuration](../getting-started/project-config.md).
- Native addons require the supported [Node-API host](../internals/node-api-host.md), explicit authorization and a matching target binary. V8/NAN/Node C++ addons are not interchangeable with Node-API addons. Dynamic loading remains constrained by the compiled graph.

## Workarounds

### Type Narrowing

Since there's no runtime type checking, use explicit checks:

```typescript
{{#include ../../examples/language/limitations.ts:type-narrowing}}
```

## Next Steps

- [Supported Features](supported-features.md) — What does work
- [Type System](type-system.md) — How types are handled
