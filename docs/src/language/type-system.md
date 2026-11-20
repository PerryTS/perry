# Type System

TypeScript annotations describe source-level intent; they do not validate runtime values. Perry combines local evidence, program facts and runtime guards to select native representations. Decorator metadata is an explicit exception to annotation erasure.

## Type Inference

Perry infers types from expressions without requiring annotations:

```typescript
{{#include ../../examples/language/type_system.ts:inference-basics}}
```

Inference works through:
- **Literal values**: `5` → `number`, `"hi"` → `string`
- **Binary operations**: `a + b` where both are numbers → `number`
- **Variable propagation**: if `x` is `number`, then `let y = x` is `number`
- **Method returns**: `"hello".trim()` → `string`, `[1,2].length` → `number`
- **Function returns**: user-defined function return types are propagated to callers

```typescript
{{#include ../../examples/language/type_system.ts:inference-function}}
```

## Type Annotations

Standard TypeScript annotations work:

```typescript
{{#include ../../examples/language/type_system.ts:annotations}}
```

## Utility Types

Utility types describe static relationships and do not introduce runtime wrappers or validation:

```typescript
{{#include ../../examples/language/type_system.ts:utility-types}}
```

These are all recognized and erased — they won't cause compilation errors.

## Generics

Generic type parameters are erased:

```typescript
{{#include ../../examples/language/type_system.ts:generics}}
```

A generic parameter alone is not a runtime representation proof. Generic value boundaries use NaN-boxing, while proven computations can use native scalars and specialized storage.

## Type Checking with `--type-check`

For stricter type checking, Perry can integrate with Microsoft's TypeScript checker:

```bash
perry file.ts --type-check
```

This starts `tsgo`, loads the project tsconfig and requests diagnostics and resolved types over IPC. Reported type errors stop compilation. Missing tooling, project-load failures or unavailable diagnostics can leave compilation running without checking; this option is not a guaranteed standalone type-check gate.

Without `--type-check`, Perry relies on its own inference engine, which handles common patterns but doesn't perform full TypeScript type checking.

## Union and Intersection Types

Unions and intersections do not create runtime tags of their own. Specialization requires evidence about the actual value, such as a validated guard:

```typescript
{{#include ../../examples/language/type_system.ts:union-narrowing}}
```

Use `typeof` checks for runtime type narrowing.

## Type Guards

```typescript
{{#include ../../examples/language/type_system.ts:type-guards}}
```

The `value is string` annotation is erased, but the `typeof` check works at runtime.

## Program facts and specialization

The default facts pass analyzes value flow across modules. It can prove receiver sets, field layouts and call targets; unknown behavior retains checks or the generic path. See [Program facts](../internals/program-facts.md) and [Local binding type evidence](../internals/local-binding-type-evidence.md).

## Next Steps

- [Supported Features](supported-features.md) — Complete feature list
- [Limitations](limitations.md) — What's not supported
