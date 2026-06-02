# parallelMap

**Signature**: `parallelMap<T, U>(data: T[], fn: (item: T) => U): U[]` — imported from `perry/thread`.

Processes every element of an array in parallel across all available CPU cores. Returns a new array with the results in the same order as the input.

## Basic Usage

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:parallel-map-basic}}
```

## How It Works

```
Input: [a, b, c, d, e, f, g, h]     (8 elements, 4 CPU cores)

  Core 1: [a, b] → map → [a', b']
  Core 2: [c, d] → map → [c', d']
  Core 3: [e, f] → map → [e', f']
  Core 4: [g, h] → map → [g', h']

Output: [a', b', c', d', e', f', g', h']   (same order as input)
```

Perry automatically detects the number of CPU cores and splits the array into equal chunks. Elements within each chunk are processed sequentially; chunks run concurrently across cores.

## Capturing Variables

The mapping function can reference variables from the outer scope. Captured values are deep-copied to each worker thread automatically:

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:parallel-map-capture}}
```

### What Can Be Captured

| Type | Supported | Transfer |
|------|-----------|----------|
| Numbers | Yes | Zero-cost (64-bit copy) |
| Booleans | Yes | Zero-cost |
| Strings | Yes | Byte copy |
| Arrays | Yes | Deep copy |
| Objects | Yes | Deep copy |
| `const` variables | Yes | Copied |
| `let`/`var` variables | Only if not reassigned | Copied |

Numeric fds and `fs.promises.FileHandle` objects are thread-affine. A captured fd
is not registered in worker threads, and a captured `FileHandle` is detached
with `fd === -1`. For file-backed parallel work, capture path strings and open
the file inside the mapper.

### What Cannot Be Captured

Mutable variables — variables that are reassigned anywhere in the enclosing scope — are rejected at compile time:

```text
// Reject example — Perry rejects this at compile time:

let total = 0;

// COMPILE ERROR: Cannot capture mutable variable 'total'
parallelMap(data, (item) => {
    total += item;   // Would be a data race
    return item;
});
```

Instead, return values and reduce:

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:parallel-map-reduce}}
```

## Performance

### When to Use parallelMap

Use `parallelMap` when the computation per element is **significantly heavier** than the cost of copying the element across threads.

**Good candidates** (CPU-bound work per element):

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:parallel-map-good-candidates}}
```

**Poor candidates** (trivial work per element):

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:parallel-map-poor-candidate}}
```

### Small Array Optimization

For arrays with fewer elements than CPU cores, Perry skips threading entirely and processes elements inline on the main thread. There's zero overhead for small inputs.

### Numeric Fast Path

When elements are pure numbers (no strings, objects, or arrays), Perry transfers them between threads at virtually zero cost — just 64-bit value copies with no serialization.

## Examples

### Matrix Row Processing

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:parallel-map-matrix}}
```

### Batch Validation

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:parallel-map-validation}}
```

### Financial Calculations

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:parallel-map-monte-carlo}}
```
