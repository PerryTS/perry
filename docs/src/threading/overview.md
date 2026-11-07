# Multi-Threading

Use `parallelMap`, `parallelFilter` and `spawn` to run native work on OS threads.

```typescript
{{#include ../../examples/runtime/thread_snippets.ts:overview-header}}
```

Perry workers run native code on OS threads, each with its own arena and collector. Ordinary captures and results are serialized between heaps; this has a cost proportional to the transferred data. SharedArrayBuffer and supported ownership transfers are explicit exceptions, not permission to share arbitrary heap pointers.

## BroadcastChannel names

`BroadcastChannel` requires a name argument. Explicit `undefined` is a valid name
and becomes `"undefined"`; omission throws `TypeError` with `ERR_MISSING_ARGS`.
Names use implicit string conversion, including a string hint for object
conversion. Symbols (including an object's conversion result) throw before a
channel is registered. A throwing conversion propagates its original value.

`BroadcastChannel.prototype` owns the read-only `name` getter and the core
`postMessage`, `close`, `ref` and `unref` methods. Borrowing one channel's method
operates on the actual receiver, including a transparent proxy or an object
inheriting from a genuine channel;
other receivers throw `TypeError` with `ERR_INVALID_THIS`. Closing a channel does
not discard its name. Receiver validation precedes message-argument validation,
and a closed channel rejects a supplied message before cloning it. Listener and
handler APIs are separate from this core-operation surface.

## Message events

`MessageEvent` extends `Event`. Its constructor accepts `data`, `origin`,
`lastEventId`, a branded `MessagePort` as `source`, and an iterable of branded
ports. Worker and channel message listeners receive the same canonical event
values, with dispatch targets and phases restored after callbacks return or throw.
Like Node 26, `initMessageEvent` returns a new event rather than mutating its receiver.

## Transferring MessagePorts

Include every `MessagePort` reachable from a message in its transfer list:

```typescript,no-test
const channel = new MessageChannel();
worker.postMessage({ port: channel.port1 }, [channel.port1]);
```

The receiving agent gets a new wrapper around the same native endpoint, including
its pending FIFO messages. The source wrapper becomes inert; moving the endpoint
back creates another wrapper rather than reactivating either old one. Listeners
stay with their original wrappers. This applies to channel ports and a Worker's
`parentPort`; transferring `parentPort` moves its data connection, not Worker
termination or reload control.

`postMessage`, `workerData` and `structuredClone` share the clone transaction.
Graph aliases, cycles, Map/Set references and transferred buffer views retain
identity. Admission and serialization must succeed before any port ownership or
buffer backing moves. A getter's successful nested transfer remains committed
when an outer transfer fails.

Delivered `MessageEvent.ports` is frozen, ordered by the transfer list, and
includes ports absent from `data`. Its entries are the same wrappers referenced
by `data`; Node-style `on("message")` and synchronous receive still return only
the message data.

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

Compile-time admission checks restrict worker closures and module-global access. Shared memory still requires synchronization, and native integrations must respect thread ownership.

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

Perry workers run native code on OS threads, each with its own arena and collector. Ordinary captures and results are serialized between heaps; this has a cost proportional to the transferred data. SharedArrayBuffer and supported ownership transfers are explicit exceptions, not permission to share arbitrary heap pointers.

File-system descriptors are also thread-affine. Numeric fds from `fs.openSync`
are just copied numbers in another thread, where the fd registry does not know
them, so fd operations fail with `EBADF`. `fs.promises.FileHandle` objects cross
thread boundaries as detached handles with `fd === -1`. Pass file paths to
`spawn`/`parallelMap` and reopen files inside the worker when it needs file I/O.

## How It Works

Perry's threading model is built on three pillars:

**1. Native Code**

Perry compiles TypeScript to native machine code via LLVM. Native worker execution still requires the ownership and transfer rules described above.

**2. Thread-Local Memory**

Each worker has its own arena and collector. This avoids a shared JavaScript heap; explicit shared memory and runtime coordination still require synchronization.

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

## Node.js Worker Constructor Compatibility

For `node:worker_threads`, a truthy `Worker` `name` must be a string; invalid
names throw `TypeError` with `ERR_INVALID_ARG_TYPE` before transferring data.
Falsy names use the default. `transferList` is validated even when `workerData`
is undefined, and listed ArrayBuffers are detached after a successful clone
whether or not the data references them.

`setEnvironmentData` retains the caller's value until it is replaced or deleted
by setting `undefined`. A new worker receives a creation-time clone of that map
alongside `workerData`; shared references and cycles are preserved within this
snapshot, while later parent changes remain local. Captured namespace methods
and CommonJS imports use the same storage as named imports. If cloning either
data source fails, construction throws before transferring any buffers.
Clone and transfer marks are private, thread-local identity metadata, not object
properties. Primitive marks are ignored. Clone marks reject ordinary objects and
Errors in both global `structuredClone` and worker messages; builtin clone kinds
such as arrays, Maps, Sets and buffers retain their own serialization behavior.
Marked transfer buffers are rejected without detachment. The marker table does
not keep its targets alive.
Inside a Node-compatible Worker, process-wide mutations such as `abort`, `chdir`,
credential setters, IPC send/disconnect and setting `umask` are rejected before
OS effects. IPC members remain absent unless enabled by the Worker startup
environment. Disabled process callables expose `disabled === true`; reading
`umask` remains allowed. Worker title writes remain local to the existing title
value, and mutable CommonJS/default reads observe the updated title. Named ESM
title imports remain a known parity limitation.
Worker eval source must be a primitive string. A URL with truthy `eval` throws
before reading workerData or detaching buffers; ordinary file URLs use the same
registered entry table as dynamically selected files. Literal string eval remains
compiled ahead of time; dynamically generated source is still unsupported.
Worker `argv` values are converted in constructor order and installed in the
child's existing process-argv authority, after execPath and the Worker entry name.
MessagePort and BroadcastChannel property reads return stored values without
invoking handlers; method calls retain their receiver through ordinary property lookup.
Message methods accept transfer iterables directly or through `options.transfer`.
The complete sequence is consumed before transfer entries are validated; iterator
errors retain their identity, and rejection leaves ArrayBuffers attached.
BroadcastChannel ignores its extra argument. Worker constructor transfer admission
remains separate; MessagePort endpoint movement and direct-message routing/timeout
parity are not provided by this normalization.
Main-thread `postMessageToThread` rejects missing or stopped destinations before
cloning the payload or detaching transfer buffers. Worker-origin messages retain
Node's forwarding order: cloning precedes destination rejection. Admission does
not guarantee rollback if a destination exits concurrently after it was admitted.
Core-module messaging constructor exports retain the original thread-realm
MessageChannel, MessagePort and BroadcastChannel objects even after global
reassignment. Explicit CommonJS export overrides and named-export synchronization
remain separate; vm/eval global population does not replace these thread-core values.
MessagePort core methods (`postMessage`, `start`, `close`, `ref`, `unref`,
`hasRef`) operate on their actual receiver, including calls borrowed from another
port or its prototype. A private ID supplies the brand without observable getters
or proxy traps; closing a port preserves its brand without retaining a closed
runtime port in the delivery table. Listener methods and realm movement remain
separate parity work.
MessagePort `close(callback)` registers callable callbacks in the existing close
listener sequence before closing. Repeated registrations preserve listener order
and callback deduplication; non-callable callback values are ignored. This does
not change queued-message flushing, endpoint close ordering or runtime-only
fallback close-event support.
Worker event-loop utilization and CPU usage directly own their shared perf-hooks
and process result layouts. Notifications additionally retain the canonical process
namespace and its declared process-result layouts, without selecting the full realm
or unrelated perf-hooks layouts.
Worker construction schedules the creating process's `worker` notification through
its public `nextTick` method. The anonymous callback retains the exact Worker,
reads `process.emit` when invoked and returns that call's result. Scheduler and
emitter overrides therefore remain observable; no separate notification queue is
used. The Worker realm declaration includes this canonical process capability.
Other worker API parity gaps remain.

## Next Steps

- [parallelMap Reference](parallel-map.md) — detailed API and performance tips
- [parallelFilter Reference](parallel-filter.md) — parallel array filtering
- [spawn Reference](spawn.md) — background threads and Promise integration

Compiled file and eval Worker entries are independent module roots during both
module pruning and activation analysis. Dynamic imports owned by those Worker
entries remain reachable; unrelated dead dynamic imports are still pruned.

### Worker termination

Each `Worker.terminate()` call returns a distinct promise. Calls made before the
`exit` event complete with the worker's exit code after that event is delivered.
Calls made from an `exit` listener or later resolve to `undefined`.

`Worker.prototype[Symbol.asyncDispose]()` dynamically calls the receiver's
`terminate()` with no arguments and awaits it, fulfilling with `undefined`.
Each call returns a fresh promise; errors from lookup, invocation, or awaiting
reject that promise. It reuses termination's exit ordering and accepts borrowed
receivers with a callable `terminate`, including overrides.

### Worker process exit

Inside a Worker, `process.exit()` stops only that Worker; it does not terminate
the parent process or resume enclosing `catch`/`finally` blocks. Explicit and
natural exits use the Worker's own `process.exitCode` and synchronous `exit`
listeners, including a listener's updated exit code. External `terminate()`
still reports code1 without running those listeners. Main-thread process exit
and finalization are unchanged.

### Message argument presence

`Worker.postMessage`, `MessagePort.postMessage` and
`BroadcastChannel.postMessage` require a message argument: an omitted argument
throws `TypeError` with code `ERR_MISSING_ARGS`, but explicit `undefined` is a
valid message. Rejected calls do not enqueue a message. Worker method calls use
the ordinary method lookup, including own replacements and getters.

For Node-compatible DOM-style delivery on BroadcastChannel and MessagePort
(including `parentPort`), an explicit `undefined` becomes `null` in
`MessageEvent.data`, matching the public constructor default. Raw
`on("message")` callbacks and `receiveMessageOnPort` retain `undefined`.
Perry-only Worker web-style and worker-global delivery keep their existing
lossless data behavior.

### BroadcastChannel EventTarget listeners

BroadcastChannel inherits EventTarget. Borrowed `addEventListener` and
`removeEventListener` use the actual receiver, including its prototype, derived
objects and transparent proxies; invalid receivers throw `ERR_INVALID_THIS`.
Listeners share EventTarget's capture matching, `once`, AbortSignal removal and
listener-object `handleEvent` dispatch. `events.getEventListeners` observes the
same registrations used by delivery and manual `dispatchEvent`.

`onmessage` and `onmessageerror` occupy stable positions in that listener list.
Replacing a handler keeps its position; assigning `null` silences the wrapper
without removing it. Closing a channel discards its transport inbox, not its
EventTarget listeners: manual dispatch remains available after close. The provider
releases the closed transport immediately; only ordinary references to the channel
retain its listeners.

Dispatch supports callable Proxy listeners and removes `once` records before
invocation, even if another callback changes the receiver's constructor. A listener
removed and re-added during dispatch does not run from the old snapshot. Persistent
listener traversal is linear in the listener count. Exceptions from callbacks or
`handleEvent` getters are reported asynchronously through `uncaughtException`;
they do not skip later listeners or property handlers.

### Messaging constructor errors

`MessageChannel` requires `new` (`ERR_CONSTRUCT_CALL_REQUIRED`). `MessagePort`
instances come from channels or transfers; calling or constructing one directly
throws `ERR_CONSTRUCT_CALL_INVALID`. Calling `BroadcastChannel` without `new`
throws the ordinary class-constructor `TypeError`, without a Node error code.

### Renamed constructor imports

Renaming a `node:worker_threads` constructor import keeps the original
constructor identity, including aliases named like another constructor.
Worker aliases retain child-entry discovery and the same options/eval lowering
as canonical imports. Lexical bindings, namespace imports and explicit global
constructors keep their existing resolution rules.

Worker module functions can be captured from named imports or CommonJS and
invoked through `call`, `apply` or spread arguments. These callable values use
the same environment, clone-marker, `receiveMessageOnPort` and
`postMessageToThread` implementations as direct calls. Realm movement through
`moveMessagePortToContext` remains unsupported on this callable path.
