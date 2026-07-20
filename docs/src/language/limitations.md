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

## No eval() or Dynamic Code

Perry compiles to native code ahead of time. It cannot evaluate a code string
that is only known at runtime. A *constant* code string is the exception —
`eval("1 + 1")` and `new Function("a", "b", "return a + b")` are compiled to
real native functions (#1679). Only a body built from runtime data hits this
limit:

```ts
// Constant body → compiled natively (works)
const add = new Function("a", "b", "return a + b");

// Runtime-built body → cannot be compiled ahead of time
function run(src: string) { return new Function(`return (${src})`)(); }
```

### Default: deferred runtime error + compile-time notice (#5206)

By default a runtime-unknown `eval(...)` / `new Function(<dynamic body>)` site
does **not** block the build. Perry compiles it to a value that throws a
descriptive `Error` *only if it is actually reached* (an `eval(...)` throws when
evaluated; a `new Function(...)` returns a function that throws when called),
and prints a single end-of-compile notice listing every degraded site:

```text
notice: 3 ahead-of-time-unsupported site(s) compiled to a deferred runtime error (throws only if reached):
  - eval(...)           src/foo.ts:12
  - import(...)          src/plugins/loader.ts:88
  - new Function(...)    src/cli/cmd/debug/agent.handler.ts:41
  Pass --strict-eval/--strict-dynamic-import (or set perry.strict = true) to make these a compile-time error instead.
```

This lets a single such call in a cold path ship without aborting the whole
build, while still failing loudly (and catchably) if that path runs.

### Dynamic `import()` with a runtime-computed specifier (#5230)

A dynamic `import(spec)` whose `spec` is only known at runtime (a plugin loader
building a path from a variable) is subject to the **same defer/notice/strict
policy** as `eval`. By default it compiles to a rejected `Promise` carrying a
descriptive `Error` (so `await import(spec)` throws *only if reached*), is
listed in the shared notice above under the `import(...)` kind, and does **not**
abort the build. This lets an app with a plugin-loader path compile and run its
core, with only the plugin-load path throwing if exercised.

Resolvable specifiers are unaffected and still compile + load: string literals
(`import("./mod.js")`), ternaries of resolvable arms, template literals over
`const` locals (`` import(`./${KIND}.js`) ``), finite string-literal-union
parameters, and directory globs.

```ts
// Resolvable → compiled + loaded as today.
const real = await import("./real.js");

// Runtime-computed → deferred (default): throws only if this line runs.
async function loadPlugin(name: string) {
  return await import(name + ".js");
}
```

### Strict mode: refuse at compile time

To make every runtime-unknown site a hard compile-time error instead, opt into
strict-eval mode by any of:

- the `--strict-eval` flag on `perry compile`,
- `"perry": { "eval": "error" }` (or `"perry": { "strict": true }`) in
  `package.json`, or
- `[perry]` `eval = "error"` (or `strict = true`) in `perry.toml`.

`perry.eval` accepts `"defer"` (the default) or `"error"`. Precedence is
package.json/perry.toml config → `--strict-eval` (opts in). The legacy
`PERRY_ALLOW_EVAL=1` environment variable still works: it forces non-strict
(defer) mode for a one-off build, overriding any strict flag/config.

The same strict controls apply to runtime-computed dynamic `import()` (#5230).
The broad `perry.strict = true` covers both eval and dynamic import. For a knob
scoped to dynamic imports only, use `--strict-dynamic-import` or
`"perry": { "dynamicImport": "error" }` (accepts `"defer"` (default) or
`"error"`); the dedicated knob overrides the broad `perry.strict` for import
sites. `PERRY_ALLOW_EVAL=1` is the shared AOT escape hatch — it forces defer for
both eval and dynamic import.

Test262 rows that only observe parsing or executing a code string remain
intentional AOT exclusions, not runtime dynamic-code work. This includes the
`language/white-space/comment-{multi,single}-{form-feed,horizontal-tab,nbsp,space,vertical-tab}.js`
rows and the direct-eval reference row `language/types/reference/8.7.2-1-s.js`;
they map to the AOT eval tracker (#1677), eval classifier diagnostics (#1678),
and the limited literal `Function` folding work (#1679).

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

## No User-Space CommonJS require()

Use static ESM imports in Perry source:

<!-- intentionally-rejects: the `require` and dynamic-`import` lines are code Perry refuses to compile -->
```text
// Supported
import { foo } from "./module";

// Not supported
const mod = require("./module");
const mod = await import("./module");
```

Perry has internal CommonJS compatibility paths for some npm package wrappers,
but user-written modules should use static `import` declarations.

> **JavaScript source compiles too.** Perry accepts `.js`, `.cjs`, `.mjs`, and
> `.jsx` files as compiler input — they are parsed as JavaScript and lowered
> through the same native pipeline as TypeScript, so no type annotations are
> required. The limitations on this page still apply (no `eval`, no general
> dynamic `require()`, etc.), but plain JavaScript projects compile and run in
> most cases.

## Prototype Manipulation

Dynamic prototype manipulation is supported for the common patterns:

```ts
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

## Weak References Retain Their Targets

`WeakMap`, `WeakSet`, `WeakRef`, and `FinalizationRegistry` are implemented and
their APIs behave as expected — `set` / `get` / `has` / `delete`, `add`,
`deref()`, and `register` / `unregister` all work and return the right values.
`WeakMap` and `WeakSet` use **reference** equality, so two distinct objects
never collide on the same slot.

The one caveat is that Perry's garbage collector does not yet treat these
references as *weak*, so targets are **retained rather than collected**. The
current runtime stores `WeakRef` targets and `FinalizationRegistry`
registrations in ordinary object/array fields (`crates/perry-runtime/src/weakref.rs`),
and the adjacent GC root scanners do not have a weak-slot clearing/finalizer
queue hook yet. In practice:

- `WeakRef.deref()` always returns the original target (it is never reported as
  collected).
- `FinalizationRegistry` records registrations but never fires its cleanup
  callback.
- `WeakMap` / `WeakSet` keep their keys alive (they behave like a
  reference-keyed `Map` / `Set`).

This is safe for **correctness** — code that reads through these APIs gets the
right values. It only matters if you depend on collection *timing* to reclaim
memory or to run finalizer side effects.

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

Perry ships **no WebAssembly engine** in the default build. The current
policy: **WASM-dependent features degrade gracefully**; full support is
tracked in [#6558](https://github.com/PerryTS/perry/issues/6558).

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
- **Not supported**: Packages requiring native addons (`.node` files), `eval()`, dynamic `require()`, or Node.js internals.

## Workarounds

### Dynamic Behavior

For cases where you need dynamic behavior, use the JavaScript runtime fallback:

<!-- intentionally-rejects: `jsEval` is a hypothetical helper used to illustrate the QuickJS escape-hatch shape; not a stable API -->
```text
import { jsEval } from "perry/jsruntime";
// Routes specific code through QuickJS for dynamic evaluation
```

### Type Narrowing

Since there's no runtime type checking, use explicit checks:

```typescript
{{#include ../../examples/language/limitations.ts:type-narrowing}}
```

## Next Steps

- [Supported Features](supported-features.md) — What does work
- [Type System](type-system.md) — How types are handled
