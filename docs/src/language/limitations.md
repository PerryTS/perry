# Limitations

Perry compiles a practical subset of TypeScript. This page documents what's not supported or works differently from Node.js/tsc.

## No Runtime Type Checking

Types are erased at compile time. There is no runtime type system — Perry doesn't generate type guards or runtime type metadata.

```typescript
{{#include ../../examples/language/limitations.ts:erased-types}}
```

Use explicit `typeof` checks where runtime type discrimination is needed.

## No eval() or Dynamic Code

Perry compiles to native code ahead of time. Dynamic code execution is not possible:

<!-- intentionally-rejects: this snippet documents code Perry refuses to compile -->
```text
// Not supported
eval("console.log('hi')");
new Function("return 42");
```

## Decorators

Perry parses decorator syntax and supports compile-time-only transforms
(see the bundled `@log` example), but does not implement the runtime
metadata facilities (`Reflect.metadata`, `Symbol`-keyed metadata,
`emitDecoratorMetadata` type capture) that Angular / NestJS / TypeORM
DI containers rely on. See [Decorators](decorators.md) for the full
stance and a worked migration recipe.

## No Runtime Metadata Reflection

TypeScript-style runtime metadata is not supported:

<!-- intentionally-rejects: this snippet documents code Perry refuses to compile -->
```text
// Not supported
Reflect.getMetadata("design:type", target, key);
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

## Limited Prototype Manipulation

Perry compiles classes to fixed structures. Dynamic prototype modification is not supported:

<!-- intentionally-rejects: this snippet documents code Perry refuses to compile -->
```text
// Not supported
MyClass.prototype.newMethod = function() {};
Object.setPrototypeOf(obj, proto);
```

`Object.getPrototypeOf(...)` and `Reflect.getPrototypeOf(...)` are supported
for class/prototype inspection patterns, but `Object.setPrototypeOf(...)` /
`Reflect.setPrototypeOf(...)` do not mutate Perry's fixed class layout.

## Weak References Are Not GC-Accurate

`WeakMap`, `WeakSet`, `WeakRef`, and `FinalizationRegistry` expose the expected
API shape, but their weak-reference semantics are pragmatic, not GC-accurate:
`WeakRef` keeps a strong reference internally, and `FinalizationRegistry`
records registrations but does not run cleanup callbacks after collection.

## Limited Proxy Trapping

Proxy support is not a full engine-level trap layer for every possible dynamic
object access. Prefer plain objects and explicit APIs unless a package only
needs Perry's supported Proxy surface.

## Threading Model

Perry supports real multi-threading via `parallelMap` and `spawn` from `perry/thread`. See [Multi-Threading](../threading/overview.md).

Threads do not share mutable state — closures passed to thread primitives cannot capture mutable variables (enforced at compile time). Values are deep-copied across thread boundaries. There is no `SharedArrayBuffer` or `Atomics`.

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
