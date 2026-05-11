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

## Symbol

```typescript
{{#include ../../examples/language/supported_features.ts:symbol}}
```

Supported Symbol features include `Symbol()`, `Symbol.for()`,
`Symbol.keyFor()`, `typeof value === "symbol"`, symbol-keyed properties,
well-known symbols such as `Symbol.iterator` and `Symbol.toPrimitive`, and
`Object.getOwnPropertySymbols()`.

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
`crates/perry-hir/src/jsx.rs`), but the runtime `_jsx` / `_jsxs` symbols are
not yet linked, so a `.tsx` file fails at the link stage today. The
canonical pattern is the function-call form Perry's UI examples already use
(`Text("hi")` instead of `<Text>hi</Text>`).

```text
// Planned JSX shape (parses but doesn't link yet — issue: runtime _jsx symbols):

function Greeting({ name }: { name: string }) {
  return <Text>{`Hello, ${name}!`}</Text>;
}

<Button onClick={() => console.log("clicked")}>Click me</Button>

<>
  <Text>Line 1</Text>
  <Text>Line 2</Text>
</>
```

JSX elements are transformed to function calls via the `jsx()`/`jsxs()` runtime.

## Next Steps

- [Type System](type-system.md) — Type inference and checking
- [Limitations](limitations.md) — What's not supported yet
