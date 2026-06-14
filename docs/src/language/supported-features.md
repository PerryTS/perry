# Supported TypeScript Features

Perry compiles a practical subset of TypeScript to native code. This page lists what's supported.

## Primitive Types

```typescript
{{#include ../../examples/language/supported_features.ts:primitives}}
```

All primitives are represented as 64-bit NaN-boxed values at runtime.

## Variables and Constants

```typescript
{{#include ../../examples/language/supported_features.ts:variables}}
```

Perry infers types from initializers — `let x = 5` is inferred as `number` without an explicit annotation.

## Functions

```typescript
{{#include ../../examples/language/supported_features.ts:functions}}
```

## Classes

```typescript
{{#include ../../examples/language/supported_features.ts:classes}}
```

Supported class features:
- Constructors
- Instance and static methods
- Instance and static properties
- Inheritance (`extends`)
- Method overriding
- `instanceof` checks (via class ID chain)
- Singleton patterns (static method return type inference)

## Enums

```typescript
{{#include ../../examples/language/supported_features.ts:enums}}
```

Enums are compiled to constants and work across modules.

## Interfaces and Type Aliases

```typescript
{{#include ../../examples/language/supported_features.ts:interfaces}}
```

Interfaces and type aliases are erased at compile time (like `tsc`). They exist only for documentation and editor tooling.

## Arrays

```typescript
{{#include ../../examples/language/supported_features.ts:arrays}}
```

## Objects

```typescript
{{#include ../../examples/language/supported_features.ts:objects}}
```

## Destructuring

```typescript
{{#include ../../examples/language/supported_features.ts:destructuring}}
```

## Template Literals

```typescript
{{#include ../../examples/language/supported_features.ts:template-literals}}
```

## Spread and Rest

```typescript
{{#include ../../examples/language/supported_features.ts:spread-rest}}
```

## Closures

```typescript
{{#include ../../examples/language/supported_features.ts:closures}}
```

Perry performs closure conversion — captured variables are stored in heap-allocated closure objects.

## Async/Await

```typescript
{{#include ../../examples/language/supported_features.ts:async-await}}
```

Perry compiles async functions to a state machine backed by Tokio's async runtime.

## Promises

```typescript
{{#include ../../examples/language/supported_features.ts:promises}}
```

## Generators

```typescript
{{#include ../../examples/language/supported_features.ts:generators}}
```

## Map and Set

```typescript
{{#include ../../examples/language/supported_features.ts:map-set}}
```

## Regular Expressions

```typescript
{{#include ../../examples/language/supported_features.ts:regex}}
```

## Error Handling

```typescript
{{#include ../../examples/language/supported_features.ts:errors}}
```

## JSON

```typescript
{{#include ../../examples/language/supported_features.ts:json}}
```

## typeof and instanceof

```typescript
{{#include ../../examples/language/supported_features.ts:typeof-instanceof}}
```

`typeof` checks NaN-boxing tags at runtime. `instanceof` walks the class ID chain.

## Modules

ES module syntax is fully supported: named exports, default exports, and
re-exports.

The exporting module:

```typescript
{{#include ../../examples/language/modules/utils.ts:exports}}
```

The importing module:

```typescript
{{#include ../../examples/language/modules/main.ts:imports}}
```

## BigInt

```typescript
{{#include ../../examples/language/supported_features.ts:bigint}}
```

## String Methods

```typescript
{{#include ../../examples/language/supported_features.ts:string-methods}}
```

## Math

```typescript
{{#include ../../examples/language/supported_features.ts:math}}
```

## Date

```typescript
{{#include ../../examples/language/supported_features.ts:date}}
```

## Console

```typescript
{{#include ../../examples/language/supported_features.ts:console}}
```

## Garbage Collection

Perry includes a mark-sweep garbage collector. It runs automatically when memory pressure is detected (~8MB arena blocks), but you can also trigger it manually:

```typescript
{{#include ../../examples/language/supported_features.ts:gc}}
```

The GC uses conservative stack scanning to find roots and supports arena-allocated objects (arrays, objects) and malloc-allocated objects (strings, closures, promises, BigInts, errors).

## JSX/TSX

Perry's parser and HIR understand JSX syntax (parsed via SWC, lowered in
`crates/perry-hir/src/jsx.rs`) and `.tsx` files link through Perry's built-in
`jsx()` / `jsxs()` runtime path. You do not need a local
`react/jsx-runtime` package just to compile TSX.

```tsx
import { Box, Text } from "perry/tui";

function Greeting({ name }: { name: string }) {
  return <Text>{`Hello, ${name}!`}</Text>;
}

const page = <div className="card"><Greeting name="Perry" /></div>;
const app = <Box><Greeting name="TUI" /></Box>;
```

JSX elements are transformed to function calls via the `jsx()` / `jsxs()`
runtime. Perry's built-in adapter supports HTML-style intrinsic tags,
fragments, function components, and compile-time rewrites for `perry/tui`
`Box` / `Text` so those TUI JSX forms lower to the same native builders as the
function-call form.

Caveat: this is Perry's TSX runtime, not React DOM or full React reconciler
semantics. For `perry/ui`, or for `perry/tui` intrinsics whose JSX rewrite has
not landed yet, the function-call form remains the canonical native API.

## JavaScript (`.js`) Input

Perry is a TypeScript compiler, but TypeScript is a superset of JavaScript — so
Perry also compiles plain JavaScript. `.js`, `.cjs`, `.mjs`, and `.jsx` files are
parsed as JavaScript (decorators, JSX, and import attributes enabled) and lowered
through the exact same native pipeline as `.ts`. No type annotations are required.

```bash
perry compile src/main.js -o myapp
./myapp
```

There are no guarantees for every dynamic JavaScript pattern (the
[Limitations](limitations.md) still apply — no `eval`, no general dynamic
`require()`), but most plain JavaScript projects compile and run.

## Node.js Compatibility

Perry implements a large, real (non-stub) slice of the Node.js standard library —
`fs`, `http`/`https`/`http2`, `net`/`tls`, `dns`/`dgram`, `crypto`, `stream`
(+ `stream/web`), `events`, `child_process`, `cluster`, `worker_threads`, `zlib`,
`process`, `async_hooks` / `AsyncLocalStorage`, `Atomics` / `SharedArrayBuffer`,
and the WHATWG web globals (`fetch`, `URL`, streams, `structuredClone`,
WebCrypto, …). Against Node's own test suite (node v26, 53 `node:*` modules)
Perry passes ~97% of cases, with overall Node/TypeScript compatibility around
95%. The per-module surface and remaining gaps are tracked in
`docs/runtime-parity-gaps.md`.

## Next Steps

- [Type System](type-system.md) — Type inference and checking
- [Limitations](limitations.md) — What's not supported yet
