# Multi-Threading

Perry gives you real OS threads with a one-line API. No worker setup, no message ports, no structured clone overhead. Just `parallelMap`, `parallelFilter`, and `spawn`.

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-header}}
```

This is something **no JavaScript runtime can do**. V8, Bun, and Deno are all locked to one thread per isolate. Perry compiles to native code — there are no isolates, no GIL, no structural limitations. Your code runs on real OS threads with the full power of every CPU core.

## Why This Matters

JavaScript's single-threaded model is its biggest performance bottleneck. Here's how runtimes try to work around it:

| Runtime | "Multi-threading" | Reality |
|---------|-------------------|---------|
| **Node.js** | `worker_threads` | Separate V8 isolates. Data copied via structured clone. ~2MB RAM per worker. Complex API. |
| **Deno** | `Worker` | Same as Node — isolated heaps, message passing only. |
| **Bun** | `Worker` | Same architecture. Faster structured clone, still isolated. |
| **Perry** | `parallelMap` / `spawn` | Real OS threads. Lightweight (8MB stack). One-line API. Compile-time safety. |

The fundamental problem: V8 uses a garbage-collected heap that **cannot be shared** between threads. Every "worker" is an entirely separate JavaScript engine instance with its own heap, its own GC, and its own copy of your data.

Perry doesn't have this limitation. It compiles TypeScript to native machine code. Values are transferred between threads using zero-cost copies for numbers and efficient serialization for objects — no separate engine instances, no multi-megabyte overhead per thread.

## Three Primitives

### `parallelMap` — Data-Parallel Processing

Split an array across all CPU cores. Each element is processed independently. Results are collected in order.

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-parallel-map}}
```

Perry automatically:
1. Detects the number of CPU cores
2. Splits the array into chunks (one per core)
3. Spawns OS threads to process each chunk
4. Collects results in the original order
5. Returns a new array

For small arrays, Perry skips threading entirely and processes inline — no overhead for trivial cases.

### `parallelFilter` — Data-Parallel Filtering

Filter a large array across all CPU cores. Like `.filter()` but parallel:

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-parallel-filter}}
```

Same rules as `parallelMap`: closures cannot capture mutable variables (compile-time enforced), and values are deep-copied between threads.

### `spawn` — Background Threads

Run any computation in the background and get a Promise back. The main thread continues immediately.

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-spawn-bg}}
```

`spawn` returns a standard Promise. You can `await` it, pass it to `Promise.all`, or chain `.then()` — it works exactly like any other async operation.

## Practical Examples

### Parallel Image Processing

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-image}}
```

### Parallel Cryptographic Hashing

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-crypto}}
```

### Multiple Independent Computations

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-multiple}}
```

### Keeping UI Responsive

```typescript
{{#include ../../examples/ui/threading/snippets.ts:ui-keep-responsive}}
```

### Captured Variables

Closures can capture outer variables. Captured values are automatically deep-copied to each worker thread:

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-captured}}
```

Numbers and booleans are zero-cost copies (just 64-bit values). Strings, arrays, and objects are deep-copied automatically.

## Safety

Perry enforces thread safety **at compile time**. You don't need to think about race conditions, mutexes, or data corruption.

### No Shared Mutable State

Closures passed to `parallelMap` and `spawn` **cannot capture mutable variables**. The compiler rejects this:

```text
// Reject example — Perry rejects this at compile time:

let counter = 0;

// COMPILE ERROR: Closures passed to parallelMap cannot
// capture mutable variable 'counter'
parallelMap(data, (item) => {
    counter++;  // Not allowed
    return item;
});
```

This eliminates data races by design. If you need to aggregate results, use the return values:

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-reduce-instead}}
```

The one explicit shared-state escape hatch is `SharedArrayBuffer`: a SAB captured
into a `spawn` / `parallelMap` closure aliases the same physical bytes across
agents, and the `Atomics` API (including a real blocking `Atomics.wait` /
`Atomics.notify` / `Atomics.waitAsync`) operates on it for cross-thread
coordination. Only the `SharedArrayBuffer` itself is shared — build any typed-array
view over it per-agent rather than capturing the view directly.

### Workers Are Synchronous

The closure passed to `parallelMap`, `parallelFilter`, or `spawn` cannot be
`async`, contain `await` (directly or in a nested closure), or call another
thread primitive — the compiler rejects all three. Async machinery is pumped
by whichever thread runs it, so a worker doing async work would drain
completions and timers belonging to other threads and alias their heaps.
Do the async part on the main thread and `await` the `spawn` result there —
the standard pattern shown above is unaffected.

### Module-Scope Objects Stay Home

Worker closures may read module-scope **primitives** (numbers, strings,
booleans), but not module-scope bindings that hold heap objects — object and
array literals, `const f = () => ...` helpers, `Map`/`Set`, class instances.
Module-level bindings live in process-wide slots that are read in place; they
do **not** go through the capture deep-copy, so the worker would alias the
main thread's heap. The compiler rejects such reads. Two easy fixes: bind the
value to a function-scope local first (`const copy = theGlobal;`) so the
closure captures the local and it is deep-copied, or declare module-level
helpers with `function name(...)` (static code — always fine to call from a
worker). `SharedArrayBuffer` module globals are exempt: cross-thread sharing
is their purpose.

### Independent Thread Arenas

Each worker thread has its own memory arena. Objects created on one thread can never be accessed from another thread. Values cross thread boundaries only through deep-copy serialization, which Perry handles automatically and invisibly.

File-system descriptors are also thread-affine. Numeric fds from `fs.openSync`
are just copied numbers in another thread, where the fd registry does not know
them, so fd operations fail with `EBADF`. `fs.promises.FileHandle` objects cross
thread boundaries as detached handles with `fd === -1`. Pass file paths to
`spawn`/`parallelMap` and reopen files inside the worker when it needs file I/O.

## How It Works

Perry's threading model is built on three pillars:

**1. Native Code, Not Interpreted**

Perry compiles TypeScript to native machine code via LLVM. There's no interpreter, no VM, no isolate. A function pointer is just a function pointer — it's valid on any thread.

**2. Thread-Local Memory**

Each thread gets its own memory arena (bump allocator) and garbage collector. No synchronization overhead during computation. When a thread finishes, its arena is freed automatically.

**3. Serialized Transfer**

Values crossing thread boundaries are serialized to a thread-safe intermediate format and deserialized on the target thread. The cost depends on the value type:

| Value Type | Transfer Cost |
|-----------|--------------|
| Numbers, booleans, null, undefined | Zero-cost (64-bit copy) |
| Strings | O(n) byte copy |
| Arrays | O(n) deep copy of elements |
| Objects | O(n) deep copy of fields |
| Closures | Pointer + captured values |
| `fs` numeric fds / `FileHandle` | Thread-affine; reopen by path |

For numeric workloads — the most common parallelizable tasks — the threading overhead is negligible.

## Next Steps

- [parallelMap Reference](parallel-map.md) — detailed API and performance tips
- [parallelFilter Reference](parallel-filter.md) — parallel array filtering
- [spawn Reference](spawn.md) — background threads and Promise integration
