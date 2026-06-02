# spawn

**Signature**: `spawn<T>(fn: () => T): Promise<T>` — imported from `perry/thread`.

Runs a closure on a new OS thread and returns a Promise that resolves when the thread completes. The main thread continues immediately — UI and other work are not blocked.

## Basic Usage

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:spawn-basic}}
```

## Non-Blocking

`spawn` returns immediately. The main thread doesn't wait:

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:spawn-non-blocking}}
```

Output:
```
1. Starting background work
2. Main thread continues immediately
3. Got result: <computed value>
```

## Multiple Concurrent Tasks

Spawn multiple tasks and they run truly concurrently — one OS thread per `spawn` call:

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:spawn-multiple}}
```

Unlike Node.js `worker_threads`, each `spawn` is a lightweight OS thread (~8MB stack), not a full V8 isolate (~2MB heap + startup cost).

## Capturing Variables

Like `parallelMap`, `spawn` closures can capture outer variables. They are deep-copied to the background thread:

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:spawn-capture}}
```

Mutable variables cannot be captured — this is enforced at compile time.

### File System Handles

Do not capture numeric fds or `fs.promises.FileHandle` objects for file I/O in
`spawn`. Perry's fd registry is per thread: a numeric fd captured from the main
thread is not open in the worker, and a captured `FileHandle` arrives detached
with `fd === -1`. Capture a path string instead, then call `fs.openSync` or
`fs.promises.open` inside the spawned function.

## Returning Complex Values

`spawn` can return any value type. Complex values (objects, arrays, strings) are serialized back to the main thread automatically:

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:spawn-complex-return}}
```

## UI Integration

`spawn` is ideal for keeping native UIs responsive during heavy computation:

```typescript
{{#include ../../examples/ui/threading/snippets.ts:ui-spawn-analyze}}
```

Without `spawn`, the analysis would freeze the UI. With `spawn`, the user can still scroll, tap other buttons, or navigate while the computation runs.

## Compared to Node.js worker_threads

```javascript
// ── Node.js: ~15 lines, separate file needed ──────────
// worker.js
const { parentPort, workerData } = require("worker_threads");
const result = heavyComputation(workerData);
parentPort.postMessage(result);

// main.js
const { Worker } = require("worker_threads");
const worker = new Worker("./worker.js", {
    workerData: inputData,
});
worker.on("message", (result) => {
    console.log(result);
});
worker.on("error", (err) => { /* handle */ });


// ── Perry: 1 line ─────────────────────────────────────
// const result = await spawn(() => heavyComputation(inputData));
```

No separate files. No message ports. No event handlers. No structured clone. One line.

## Examples

### Background File Processing

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:spawn-bg-file}}
```

### Parallel API Calls with Processing

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:spawn-api-then-process}}
```

### Deferred Computation

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:spawn-deferred}}
```
