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

Perry compiles to native code ahead of time. Dynamic code execution is not possible:

<!-- intentionally-rejects: this snippet documents code Perry refuses to compile -->
```text
// Not supported
eval("console.log('hi')");
new Function("return 42");
```

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

## Weak References Retain Their Targets

`WeakMap`, `WeakSet`, `WeakRef`, and `FinalizationRegistry` are implemented and
their APIs behave as expected — `set` / `get` / `has` / `delete`, `add`,
`deref()`, and `register` / `unregister` all work and return the right values.
`WeakMap` and `WeakSet` use **reference** equality, so two distinct objects
never collide on the same slot.

The one caveat is that Perry's garbage collector does not yet treat these
references as *weak*, so targets are **retained rather than collected**. In
practice:

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
