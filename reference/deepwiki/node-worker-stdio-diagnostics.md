# DeepWiki Response

Repository: `nodejs/node`
Question: What are Node worker_threads Worker option state, stdio stream, resourceLimits, and diagnostics method observable semantics?
Search: https://deepwiki.com/search/what-are-node-workerthreads-wo_93d4f93d-621d-44da-8e8f-0d64787c362e

**Question**: What are Node worker_threads Worker option state, stdio stream, resourceLimits, and diagnostics method observable semantics?

Node.js `worker_threads` provide several observable semantics for `Worker` options, stdio streams, resource limits, and diagnostic methods. These allow you to monitor and manage the behavior of worker threads.

### Worker Option State
The `Worker` constructor in `node:worker_threads` accepts an `options` object that defines the worker's initial state and behavior [1](#ref-q1-1) . Key observable options include:
*   `env`: Specifies the initial `process.env` for the worker. You can either provide a custom object or use `worker_threads.SHARE_ENV` to share the parent's environment variables [2](#ref-q1-2) . When `SHARE_ENV` is used, changes in one thread's `process.env` affect the other [3](#ref-q1-3) .
*   `execArgv`: A list of Node.js CLI options passed to the worker, available as `process.execArgv` inside the worker [4](#ref-q1-4) .
*   `workerData`: Any JavaScript value cloned and made available as `require('node:worker_threads').workerData` within the worker [5](#ref-q1-5) .
*   `name`: An optional string used for debugging and identification, which replaces the thread name and worker title [6](#ref-q1-6) .

### Stdio Streams
Worker threads can have their standard I/O streams (`stdin`, `stdout`, `stderr`) redirected or observed [7](#ref-q1-7) .
*   `worker.stdin`: A writable stream. If `stdin: true` is passed to the `Worker` constructor, data written to this stream becomes `process.stdin` in the worker [8](#ref-q1-8) .
*   `worker.stdout`: A readable stream containing data written to `process.stdout` inside the worker. If `stdout: true` is not set, it's piped to the parent's `process.stdout` [9](#ref-q1-9) .
*   `worker.stderr`: Similar to `stdout`, this readable stream contains data written to `process.stderr` in the worker [10](#ref-q1-10) .

These streams are managed internally by `ReadableWorkerStdio` and `WritableWorkerStdio` classes [11](#ref-q1-11) .

### Resource Limits
The `resourceLimits` option allows you to set constraints on the new JS engine instance within a worker [12](#ref-q1-12) . These limits affect the V8 JavaScript engine and include:
*   `maxYoungGenerationSizeMb`: Maximum size of the young generation heap in MB [13](#ref-q1-13) .
*   `maxOldGenerationSizeMb`: Maximum size of the main heap in MB [14](#ref-q1-14) .
*   `codeRangeSizeMb`: Size of a pre-allocated memory range for generated code [15](#ref-q1-15) .
*   `stackSizeMb`: Default maximum stack size for the thread [16](#ref-q1-16) .

These limits are accessible via `worker_threads.resourceLimits` within the worker thread [17](#ref-q1-17)  and `worker.resourceLimits` from the parent thread [18](#ref-q1-18) . The C++ implementation in `src/node_worker.cc` handles updating these resource constraints for the V8 engine [19](#ref-q1-19) .

### Diagnostics Methods
Node.js provides several methods to observe and diagnose worker thread behavior:
*   `worker.cpuUsage([prev])`: Returns a Promise that resolves to an object similar to `process.threadCpuUsage()`, allowing observation of CPU usage from outside the worker [20](#ref-q1-20) .
*   `worker.getHeapSnapshot([options])`: Returns a Promise for a Readable Stream containing a V8 heap snapshot of the worker's current state [21](#ref-q1-21) .
*   `worker.getHeapStatistics()`: Returns a Promise that resolves to an object identical to `v8.getHeapStatistics()`, providing heap statistics from outside the worker [22](#ref-q1-22) .
*   `worker.performance.eventLoopUtilization([utilization1[, utilization2]])`: Returns an object with `idle`, `active`, and `utilization` properties, indicating the event loop utilization of the worker [23](#ref-q1-23) . This is available after the `'online'` event [24](#ref-q1-24) .
*   `worker.startCpuProfile()`: Returns a Promise that fulfills with a `CPUProfileHandle` object, allowing you to start CPU profiling of the worker [25](#ref-q1-25) .
*   `worker.startHeapProfile()`: Similar to `startCpuProfile()`, this method starts heap profiling of the worker [26](#ref-q1-26) .

## Notes
The `Worker` class in `lib/internal/worker.js` is the JavaScript-side implementation that wraps the C++ `Worker` object defined in `src/node_worker.h` and `src/node_worker.cc` [27](#ref-q1-27) . The `typings/internalBinding/worker.d.ts` file provides TypeScript definitions for the internal worker binding, outlining the available methods and properties of the `Worker` class at a lower level [28](#ref-q1-28) .

Wiki pages you might want to explore:
- [Core Runtime Architecture (nodejs/node)](/wiki/nodejs/node#2)

View this search on DeepWiki: https://deepwiki.com/search/what-are-node-workerthreads-wo_93d4f93d-621d-44da-8e8f-0d64787c362e

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/worker_threads.md:1646-1712`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1646-L1712)

```markdown
* `options` {Object}
  * `argv` {any\[]} List of arguments which would be stringified and appended to
    `process.argv` in the worker. This is mostly similar to the `workerData`
    but the values are available on the global `process.argv` as if they
    were passed as CLI options to the script.
  * `env` {Object} If set, specifies the initial value of `process.env` inside
    the Worker thread. As a special value, [`worker.SHARE_ENV`][] may be used
    to specify that the parent thread and the child thread should share their
    environment variables; in that case, changes to one thread's `process.env`
    object affect the other thread as well. **Default:** `process.env`.
  * `eval` {boolean} If `true` and the first argument is a `string`, interpret
    the first argument to the constructor as a script that is executed once the
    worker is online.
  * `execArgv` {string\[]} List of node CLI options passed to the worker.
    V8 options (such as `--max-old-space-size`) and options that affect the
    process (such as `--title`) are not supported. If set, this is provided
    as [`process.execArgv`][] inside the worker. By default, options are
    inherited from the parent thread.
  * `stdin` {boolean} If this is set to `true`, then `worker.stdin`
    provides a writable stream whose contents appear as `process.stdin`
    inside the Worker. By default, no data is provided.
  * `stdout` {boolean} If this is set to `true`, then `worker.stdout` is
    not automatically piped through to `process.stdout` in the parent.
  * `stderr` {boolean} If this is set to `true`, then `worker.stderr` is
    not automatically piped through to `process.stderr` in the parent.
  * `workerData` {any} Any JavaScript value that is cloned and made
    available as [`require('node:worker_threads').workerData`][]. The cloning
    occurs as described in the [HTML structured clone algorithm][], and an error
    is thrown if the object cannot be cloned (e.g. because it contains
    `function`s).
  * `trackUnmanagedFds` {boolean} If this is set to `true`, then the Worker
    tracks raw file descriptors managed through [`fs.open()`][] and
    [`fs.close()`][], and closes them when the Worker exits, similar to other
    resources like network sockets or file descriptors managed through
    the [`FileHandle`][] API. This option is automatically inherited by all
    nested `Worker`s. **Default:** `true`.
  * `transferList` {Object\[]} If one or more `MessagePort`-like objects
    are passed in `workerData`, a `transferList` is required for those
    items or [`ERR_MISSING_MESSAGE_PORT_IN_TRANSFER_LIST`][] is thrown.
    See [`port.postMessage()`][] for more information.
  * `resourceLimits` {Object} An optional set of resource limits for the new JS
    engine instance. Reaching these limits leads to termination of the `Worker`
    instance. These limits only affect the JS engine, and no external data,
    including no `ArrayBuffer`s. Even if these limits are set, the process may
    still abort if it encounters a global out-of-memory situation.
    * `maxOldGenerationSizeMb` {number} The maximum size of the main heap in
      MB. If the command-line argument [`--max-old-space-size`][] is set, it
      overrides this setting.
    * `maxYoungGenerationSizeMb` {number} The maximum size of a heap space for
      recently created objects. If the command-line argument
      [`--max-semi-space-size`][] is set, it overrides this setting.
    * `codeRangeSizeMb` {number} The size of a pre-allocated memory range
      used for generated code.
    * `stackSizeMb` {number} The default maximum stack size for the thread.
      Small values may lead to unusable Worker instances. **Default:** `4`.
  * `name` {string} An optional `name` to be replaced in the thread name
    and to the worker title for debugging/identification purposes,
    making the final title as `[worker ${id}] ${name}`.
    This parameter has a maximum allowed size, depending on the operating
    system. If the provided name exceeds the limit, it will be truncated
    * Maximum sizes:
      * Windows: 32,767 characters
      * macOS: 64 characters
      * Linux: 16 characters
      * NetBSD: limited to `PTHREAD_MAX_NAMELEN_NP`
      * FreeBSD and OpenBSD: limited to `MAXCOMLEN`
        **Default:** `'WorkerThread'`.
```

<a id="ref-q1-2"></a>
### [2] `doc/api/worker_threads.md:1651-1655`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1651-L1655)

```markdown
  * `env` {Object} If set, specifies the initial value of `process.env` inside
    the Worker thread. As a special value, [`worker.SHARE_ENV`][] may be used
    to specify that the parent thread and the child thread should share their
    environment variables; in that case, changes to one thread's `process.env`
    object affect the other thread as well. **Default:** `process.env`.
```

<a id="ref-q1-3"></a>
### [3] `doc/api/worker_threads.md:665-668`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L665-L668)

```markdown
A special value that can be passed as the `env` option of the [`Worker`][]
constructor, to indicate that the current thread and the Worker thread should
share read and write access to the same set of environment variables.
```

<a id="ref-q1-4"></a>
### [4] `doc/api/worker_threads.md:1660-1663`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1660-L1663)

```markdown
    V8 options (such as `--max-old-space-size`) and options that affect the
    process (such as `--title`) are not supported. If set, this is provided
    as [`process.execArgv`][] inside the worker. By default, options are
    inherited from the parent thread.
```

<a id="ref-q1-5"></a>
### [5] `doc/api/worker_threads.md:1671-1675`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1671-L1675)

```markdown
  * `workerData` {any} Any JavaScript value that is cloned and made
    available as [`require('node:worker_threads').workerData`][]. The cloning
    occurs as described in the [HTML structured clone algorithm][], and an error
    is thrown if the object cannot be cloned (e.g. because it contains
    `function`s).
```

<a id="ref-q1-6"></a>
### [6] `doc/api/worker_threads.md:1701-1704`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1701-L1704)

```markdown
  * `name` {string} An optional `name` to be replaced in the thread name
    and to the worker title for debugging/identification purposes,
    making the final title as `[worker ${id}] ${name}`.
    This parameter has a maximum allowed size, depending on the operating
```

<a id="ref-q1-7"></a>
### [7] `doc/api/worker_threads.md:1505-1506`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1505-L1506)

```markdown
* The [`process.stdin`][], [`process.stdout`][], and [`process.stderr`][]
  streams may be redirected by the parent thread.
```

<a id="ref-q1-8"></a>
### [8] `doc/api/worker_threads.md:2072-2075`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L2072-L2075)

```markdown
If `stdin: true` was passed to the [`Worker`][] constructor, this is a
writable stream. The data written to this stream will be made available in
the worker thread as [`process.stdin`][].
```

<a id="ref-q1-9"></a>
### [9] `doc/api/worker_threads.md:2084-2087`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L2084-L2087)

```markdown
This is a readable stream which contains data written to [`process.stdout`][]
inside the worker thread. If `stdout: true` was not passed to the
[`Worker`][] constructor, then data is piped to the parent thread's
[`process.stdout`][] stream.
```

<a id="ref-q1-10"></a>
### [10] `doc/api/worker_threads.md:2059-2062`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L2059-L2062)

```markdown
This is a readable stream which contains data written to [`process.stderr`][]
inside the worker thread. If `stderr: true` was not passed to the
[`Worker`][] constructor, then data is piped to the parent thread's
[`process.stderr`][] stream.
```

<a id="ref-q1-11"></a>
### [11] `lib/internal/worker/io.js:250-284`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/worker/io.js#L250-L284)

```javascript
class ReadableWorkerStdio extends Readable {
  constructor(port, name) {
    super();
    this[kPort] = port;
    this[kName] = name;
    this[kIncrementsPortRef] = true;
    this[kStartedReading] = false;
    this.on('end', () => {
      if (this[kStartedReading] && this[kIncrementsPortRef]) {
        if (--this[kPort][kWaitingStreams] === 0)
          this[kPort].unref();
      }
    });
  }

  _read() {
    if (!this[kStartedReading] && this[kIncrementsPortRef]) {
      this[kStartedReading] = true;
      if (this[kPort][kWaitingStreams]++ === 0)
        this[kPort].ref();
    }

    this[kPort].postMessage({
      type: messageTypes.STDIO_WANTS_MORE_DATA,
      stream: this[kName],
    });
  }
}

class WritableWorkerStdio extends Writable {
  constructor(port, name) {
    super({ decodeStrings: false });
    this[kPort] = port;
    this[kName] = name;
    this[kWritableCallback] = null;
```

<a id="ref-q1-12"></a>
### [12] `doc/api/worker_threads.md:1686-1688`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1686-L1688)

```markdown
  * `resourceLimits` {Object} An optional set of resource limits for the new JS
    engine instance. Reaching these limits leads to termination of the `Worker`
    instance. These limits only affect the JS engine, and no external data,
```

<a id="ref-q1-13"></a>
### [13] `doc/api/worker_threads.md:1691-1693`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1691-L1693)

```markdown
    * `maxOldGenerationSizeMb` {number} The maximum size of the main heap in
      MB. If the command-line argument [`--max-old-space-size`][] is set, it
      overrides this setting.
```

<a id="ref-q1-14"></a>
### [14] `doc/api/worker_threads.md:1690-1691`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1690-L1691)

```markdown
    still abort if it encounters a global out-of-memory situation.
    * `maxOldGenerationSizeMb` {number} The maximum size of the main heap in
```

<a id="ref-q1-15"></a>
### [15] `doc/api/worker_threads.md:1697-1698`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1697-L1698)

```markdown
    * `codeRangeSizeMb` {number} The size of a pre-allocated memory range
      used for generated code.
```

<a id="ref-q1-16"></a>
### [16] `doc/api/worker_threads.md:1699-1700`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1699-L1700)

```markdown
    * `stackSizeMb` {number} The default maximum stack size for the thread.
      Small values may lead to unusable Worker instances. **Default:** `4`.
```

<a id="ref-q1-17"></a>
### [17] `doc/api/worker_threads.md:645-646`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L645-L646)

```markdown
* Type: {Object}
  * `maxYoungGenerationSizeMb` {number}
```

<a id="ref-q1-18"></a>
### [18] `doc/api/worker_threads.md:1957-1958`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1957-L1958)

```markdown
Provides the set of JS engine resource constraints for this Worker thread.
If the `resourceLimits` option was passed to the [`Worker`][] constructor,
```

<a id="ref-q1-19"></a>
### [19] `src/node_worker.cc:132-157`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_worker.cc#L132-L157)

```cpp
void Worker::UpdateResourceConstraints(ResourceConstraints* constraints) {
  constraints->set_stack_limit(reinterpret_cast<uint32_t*>(stack_base_));

  if (resource_limits_[kMaxYoungGenerationSizeMb] > 0) {
    constraints->set_max_young_generation_size_in_bytes(
        static_cast<size_t>(resource_limits_[kMaxYoungGenerationSizeMb] * kMB));
  } else {
    resource_limits_[kMaxYoungGenerationSizeMb] =
        constraints->max_young_generation_size_in_bytes() / kMB;
  }

  if (resource_limits_[kMaxOldGenerationSizeMb] > 0) {
    constraints->set_max_old_generation_size_in_bytes(
        static_cast<size_t>(resource_limits_[kMaxOldGenerationSizeMb] * kMB));
  } else {
    resource_limits_[kMaxOldGenerationSizeMb] =
        constraints->max_old_generation_size_in_bytes() / kMB;
  }

  if (resource_limits_[kCodeRangeSizeMb] > 0) {
    constraints->set_code_range_size_in_bytes(
        static_cast<size_t>(resource_limits_[kCodeRangeSizeMb] * kMB));
  } else {
    resource_limits_[kCodeRangeSizeMb] =
        constraints->code_range_size_in_bytes() / kMB;
  }
```

<a id="ref-q1-20"></a>
### [20] `doc/api/worker_threads.md:1783-1788`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1783-L1788)

```markdown

* Returns: {Promise}

This method returns a `Promise` that will resolve to an object identical to [`process.threadCpuUsage()`][],
or reject with an [`ERR_WORKER_NOT_RUNNING`][] error if the worker is no longer running.
This methods allows the statistics to be observed from outside the actual thread.
```

<a id="ref-q1-21"></a>
### [21] `doc/api/worker_threads.md:1807-1810`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1807-L1810)

```markdown
* Returns: {Promise} A promise for a Readable Stream containing
  a V8 heap snapshot

Returns a readable stream for a V8 snapshot of the current state of the Worker.
```

<a id="ref-q1-22"></a>
### [22] `doc/api/worker_threads.md:1827-1829`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1827-L1829)

```markdown
This method returns a `Promise` that will resolve to an object identical to [`v8.getHeapStatistics()`][],
or reject with an [`ERR_WORKER_NOT_RUNNING`][] error if the worker is no longer running.
This methods allows the statistics to be observed from outside the actual thread.
```

<a id="ref-q1-23"></a>
### [23] `doc/api/worker_threads.md:1856-1859`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1856-L1859)

```markdown
* Returns: {Object}
  * `idle` {number}
  * `active` {number}
  * `utilization` {number}
```

<a id="ref-q1-24"></a>
### [24] `doc/api/worker_threads.md:1915-1917`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1915-L1917)

```markdown
The event loop utilization of a worker is available only after the [`'online'`
event][] emitted, and if called before this, or after the [`'exit'`
event][], then all properties have the value of `0`.
```

<a id="ref-q1-25"></a>
### [25] `doc/api/worker_threads.md:1969-1971`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1969-L1971)

```markdown
* Returns: {Promise}

Starting a CPU profile then return a Promise that fulfills with an error
```

<a id="ref-q1-26"></a>
### [26] `doc/api/worker_threads.md:2006-2010`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L2006-L2010)

```markdown
### `worker.startHeapProfile()`

<!-- YAML
added:
  - v24.9.0
```

<a id="ref-q1-27"></a>
### [27] `lib/internal/worker.js:280-286`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/worker.js#L280-L286)

```javascript
    this[kHandle] = new WorkerImpl(url,
                                   env === process.env ? null : env,
                                   options.execArgv,
                                   parseResourceLimits(options.resourceLimits),
                                   !!(options.trackUnmanagedFds ?? true),
                                   isInternal,
                                   name);
```

<a id="ref-q1-28"></a>
### [28] `typings/internalBinding/worker.d.ts:4-25`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/typings/internalBinding/worker.d.ts#L4-L25)

```typescript
  class Worker {
    constructor(
      url: string | URL | null,
      env: object | null | undefined,
      execArgv: string[] | null | undefined,
      resourceLimits: Float64Array,
      trackUnmanagedFds: boolean,
    );
    startThread(): void;
    stopThread(): void;
    hasRef(): boolean;
    ref(): void;
    unref(): void;
    getResourceLimits(): Float64Array;
    takeHeapSnapshot(): object;
    getHeapStatistics(): Promise<object>;
    cpuUsage(): Promise<object>;
    startCpuProfile(): Promise<CPUProfileHandle>;
    startHeapProfile(): Promise<HeapProfileHandle>;
    loopIdleTime(): number;
    loopStartTime(): number;
  }
```
