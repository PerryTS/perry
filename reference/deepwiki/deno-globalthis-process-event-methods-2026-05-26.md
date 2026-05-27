# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility in Deno, what source files/tests establish that globalThis.process is the same process object exposed by node:process and provides EventEmitter-style methods such as on, emit, once, and off as functions? Focus only on public source/tests useful as reference for a PerryTS/perry fix where storing const gproc = globalThis.process must keep object/method shape.
Search: https://deepwiki.com/search/for-node-compatibility-in-deno_ed2e83bf-e731-4409-b867-040f33593c63

**Question**: For Node compatibility in Deno, what source files/tests establish that globalThis.process is the same process object exposed by node:process and provides EventEmitter-style methods such as on, emit, once, and off as functions? Focus only on public source/tests useful as reference for a PerryTS/perry fix where storing const gproc = globalThis.process must keep object/method shape.

The `globalThis.process` object in Deno's Node.js compatibility layer is implemented in [1](#ref-q1-1)  and [2](#ref-q1-2) . It is designed to be compatible with Node.js's `process` object, including providing EventEmitter-style methods. [3](#ref-q1-3)

### Implementation of `globalThis.process`

The `process` object is defined as a `Process` function, which internally calls `EventEmitter.call(this)` to inherit EventEmitter functionalities. [3](#ref-q1-3)  The prototype of `Process` is then set to inherit from `EventEmitter.prototype` [4](#ref-q1-4) . This ensures that `globalThis.process` has methods like `on`, `emit`, `once`, and `off`.

Specifically, the `on` method is implemented to handle signal events by wrapping the listener with `_wrapSignalListener` and adding it using `Deno.addSignalListener` [5](#ref-q1-5) . The `emit` method directly calls `EventEmitter.prototype.emit` [6](#ref-q1-6) . The `addListener` and `removeListener` methods are aliases for `on` and `off` respectively [7](#ref-q1-7) [8](#ref-q1-8) . The `removeAllListeners` method also handles signal listeners by calling `_removeAllSignalListeners` before delegating to `EventEmitter.prototype.removeAllListeners` [9](#ref-q1-9) .

The `EventEmitter` itself is loaded from `ext:deno_node/_events.mjs` [10](#ref-q1-10) .

### Tests for `globalThis.process`

The `tests/unit_node/process_test.ts` file contains several tests that verify the behavior of the `process` object and its EventEmitter-style methods. [11](#ref-q1-11)

*   **`process.on`**: This test verifies that `process.on` is a function and that an 'exit' event listener can be triggered [12](#ref-q1-12) . It also includes a test for signal handling, ensuring that `process.on("SIGINT", ...)` correctly registers a signal listener [13](#ref-q1-13) .
*   **`process.off`**: This test confirms that `process.off` can successfully remove signal listeners [14](#ref-q1-14) .
*   **`process.on, process.off, process.removeListener doesn't throw on unimplemented events`**: This test iterates through various event types, including `beforeExit`, `rejectionHandled`, `uncaughtException`, and `unhandledRejection`, to ensure that `on`, `off`, and `removeListener` do not throw errors for these events and correctly manage listener counts [15](#ref-q1-15) .
*   **`process.listeners - include SIG* events`**: This test specifically checks that `process.listeners` correctly reports the number of listeners for signal events after `on` and `prependListener` are used, and that `off` reduces the count [16](#ref-q1-16) .
*   **`signal handlers receive signal name`**: This regression test verifies that signal handlers registered with `process.once` receive the signal name as their first argument, matching Node.js behavior [17](#ref-q1-17) .

These source files and tests collectively establish that `globalThis.process` in Deno's Node.js compatibility layer is designed to mimic the Node.js `process` object, including its EventEmitter-style methods, and that its object/method shape is maintained. [3](#ref-q1-3) [12](#ref-q1-12)

## Notes

The `cli/tsc/dts/node/process.d.cts` file provides TypeScript declaration for the `process` object, including its `EventEmitter` methods, which serves as a reference for the expected shape of the `process` object. [18](#ref-q1-18)  The `tests/specs/npm/compare_globals/main.ts` file also indirectly confirms the presence and behavior of `globalThis.process` by comparing it with `npm:@denotest/globals` [19](#ref-q1-19) .

Wiki pages you might want to explore:
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-in-deno_ed2e83bf-e731-4409-b867-040f33593c63

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/process.ts:1-61`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L1-L61)

```typescript
// Copyright 2018-2026 the Deno authors. MIT license.
// Copyright Joyent, Inc. and Node.js contributors. All rights reserved. MIT license.

// TODO(petamoriken): enable prefer-primordials for node polyfills
// deno-lint-ignore-file prefer-primordials

import { core, internals, primordials } from "ext:core/mod.js";
const { initializeDebugEnv } = core.loadExtScript(
  "ext:deno_node/internal/util/debuglog.ts",
);
const { format } = core.loadExtScript(
  "ext:deno_node/internal/util/inspect.mjs",
);
import {
  op_current_thread_cpu_usage,
  op_fs_umask,
  op_getegid,
  op_geteuid,
  op_getgroups,
  op_node_load_env_file,
  op_node_process_constrained_memory,
  op_node_process_kill,
  op_node_process_set_title,
  op_node_process_setegid,
  op_node_process_seteuid,
  op_node_process_setgid,
  op_node_process_setuid,
  op_process_abort,
} from "ext:core/ops";

const { EventEmitter } = core.loadExtScript("ext:deno_node/_events.mjs");
import Module, { getBuiltinModule } from "node:module";
const { report } = core.loadExtScript(
  "ext:deno_node/internal/process/report.ts",
);
const { onWarning } = core.loadExtScript(
  "ext:deno_node/internal/process/warning.ts",
);
const {
  parseFileMode,
  validateBoolean,
  validateNumber,
  validateObject,
  validateString,
  validateUint32,
} = core.loadExtScript("ext:deno_node/internal/validators.mjs");
const {
  denoErrorToNodeError,
  ERR_INVALID_ARG_TYPE,
  ERR_INVALID_ARG_VALUE_RANGE,
  ERR_OUT_OF_RANGE,
  ERR_UNCAUGHT_EXCEPTION_CAPTURE_ALREADY_SET,
  ERR_UNKNOWN_SIGNAL,
  ERR_WORKER_UNSUPPORTED_OPERATION,
  errnoException,
  NodeTypeError,
} = core.loadExtScript("ext:deno_node/internal/errors.ts");
const { getOptionValue } = core.loadExtScript(
  "ext:deno_node/internal/options.ts",
);
const { default: assert } = core.loadExtScript("ext:deno_node/assert.ts");
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/_process/process.ts:1-79`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_process/process.ts#L1-L79)

```typescript
// Copyright 2018-2026 the Deno authors. MIT license.
// Copyright Joyent, Inc. and Node.js contributors. All rights reserved. MIT license.

// The following are all the process APIs that don't depend on the stream module
// They have to be split this way to prevent a circular dependency
(function () {
const { core, primordials } = globalThis.__bootstrap;
const {
  Error,
  ObjectGetOwnPropertyNames,
  String,
  ReflectOwnKeys,
  ArrayPrototypeIncludes,
  Object,
  Proxy,
  ObjectPrototype,
  ObjectPrototypeIsPrototypeOf,
  ReflectDefineProperty,
  ReflectHas,
  TypeError,
  TypeErrorPrototype,
} = primordials;
const { build, createLazyLoader } = core;

const { nextTick: _nextTick } = core.loadExtScript(
  "ext:deno_node/_next_tick.ts",
);
const { _exiting } = core.loadExtScript("ext:deno_node/_process/exiting.ts");
const fs = core.loadExtScript("ext:deno_fs/30_fs.js");
const {
  denoErrorToNodeError,
  ERR_INVALID_ARG_TYPE,
  ERR_INVALID_OBJECT_DEFINE_PROPERTY,
} = core.loadExtScript("ext:deno_node/internal/errors.ts");

const loadProcess = createLazyLoader<NodeJS.Process>("node:process");
let nodeProcess: NodeJS.Process | undefined;

/** Returns the operating system CPU architecture for which the Deno binary was compiled */
function arch(): string {
  if (build.arch == "x86_64") {
    return "x64";
  } else if (build.arch == "aarch64") {
    return "arm64";
  } else if (build.arch == "riscv64gc") {
    return "riscv64";
  } else {
    throw new Error("unreachable");
  }
}

/** https://nodejs.org/api/process.html#process_process_chdir_directory */
function chdir(directory: string): void {
  if (typeof directory !== "string") {
    throw new ERR_INVALID_ARG_TYPE("directory", "string", directory);
  }
  // Node's chdir error carries `path` (the cwd before chdir), `dest` (the
  // target), and `syscall: 'chdir'`. Snapshot the cwd before attempting the
  // change so the error's `path` matches Node's behaviour. If the current
  // cwd has been deleted (common in tmpdir cleanup during process exit),
  // `fs.cwd()` itself throws -- fall back to an empty string so the wrapper
  // still has a sensible `path`, and don't surface the cwd lookup error.
  let fromPath = "";
  try {
    fromPath = fs.cwd();
  } catch {
    // Ignore -- chdir() below will surface a chdir-shaped error.
  }
  try {
    fs.chdir(directory);
  } catch (err) {
    throw denoErrorToNodeError(err as Error, {
      syscall: "chdir",
      path: fromPath,
      dest: directory,
    });
  }
}
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/process.ts:567-572`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L567-L572)

```typescript
const Process = function process(this: any) {
  // deno-lint-ignore no-explicit-any
  if (!(this instanceof Process)) return new (Process as any)();

  EventEmitter.call(this);
};
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/process.ts:573`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L573)

```typescript
Process.prototype = Object.create(EventEmitter.prototype);
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/process.ts:650-673`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L650-L673)

```typescript
Process.prototype.on = function (
  // deno-lint-ignore no-explicit-any
  this: any,
  event: string,
  // deno-lint-ignore no-explicit-any
  listener: (...args: any[]) => void,
) {
  if (typeof event === "string" && event.startsWith("SIG")) {
    if (event === "SIGBREAK" && Deno.build.os !== "windows") {
      // Ignores SIGBREAK if the platform is not windows.
    } else if (event === "SIGTERM" && Deno.build.os === "windows") {
      // Ignores SIGTERM on windows.
    } else if (
      event !== "SIGBREAK" && event !== "SIGINT" &&
      event !== "SIGWINCH" && Deno.build.os === "windows"
    ) {
      // TODO(#26331): Ignores all signals except SIGBREAK, SIGINT, and SIGWINCH on windows.
    } else {
      EventEmitter.prototype.on.call(this, event, listener);
      Deno.addSignalListener(
        event as Deno.Signal,
        _wrapSignalListener(event, listener),
      );
    }
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/process.ts:716-724`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L716-L724)

```typescript
Process.prototype.emit = function (
  // deno-lint-ignore no-explicit-any
  this: any,
  event: string,
  // deno-lint-ignore no-explicit-any
  ...args: any[]
): boolean {
  return EventEmitter.prototype.emit.call(this, event, ...args);
};
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/process.ts:750-758`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L750-L758)

```typescript
Process.prototype.addListener = function (
  // deno-lint-ignore no-explicit-any
  this: any,
  event: string,
  // deno-lint-ignore no-explicit-any
  listener: (...args: any[]) => void,
) {
  return this.on(event, listener);
};
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/process.ts:760-767`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L760-L767)

```typescript
Process.prototype.removeListener = function (
  // deno-lint-ignore no-explicit-any
  this: any,
  event: string, // deno-lint-ignore no-explicit-any
  listener: (...args: any[]) => void,
) {
  return this.off(event, listener);
};
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/process.ts:769-790`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L769-L790)

```typescript
Process.prototype.removeAllListeners = function (
  // deno-lint-ignore no-explicit-any
  event?: string | any,
) {
  if (arguments.length === 0) {
    // Remove all listeners for all events - find all signal events and
    // unregister their Deno signal listeners before clearing.
    const events = this._events;
    if (events !== undefined) {
      for (const key of Object.keys(events)) {
        if (typeof key === "string" && key.startsWith("SIG")) {
          _removeAllSignalListeners(this, key);
        }
      }
    }
    return EventEmitter.prototype.removeAllListeners.call(this);
  }
  if (typeof event === "string" && event.startsWith("SIG")) {
    _removeAllSignalListeners(this, event);
  }
  return EventEmitter.prototype.removeAllListeners.call(this, event);
};
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/process.ts:31`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L31)

```typescript
const { EventEmitter } = core.loadExtScript("ext:deno_node/_events.mjs");
```

<a id="ref-q1-11"></a>
### [11] `tests/unit_node/process_test.ts:1-43`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L1-L43)

```typescript
// Copyright 2018-2026 the Deno authors. MIT license.

// deno-lint-ignore-file no-undef no-console

import process, {
  arch as importedArch,
  argv,
  argv0 as importedArgv0,
  cpuUsage as importedCpuUsage,
  env,
  execArgv as importedExecArgv,
  execPath as importedExecPath,
  getegid,
  geteuid,
  getgid,
  getuid,
  pid as importedPid,
  platform as importedPlatform,
  setegid,
  seteuid,
  setgid,
  setuid,
} from "node:process";

import { Readable } from "node:stream";
import { once } from "node:events";
import {
  assert,
  assertEquals,
  assertFalse,
  assertMatch,
  assertObjectMatch,
  assertStrictEquals,
  assertThrows,
  fail,
} from "@std/assert";
import { stripAnsiCode } from "@std/fmt/colors";
import * as path from "@std/path";
import { delay } from "@std/async/delay";
import { stub } from "@std/testing/mock";
import { execSync } from "node:child_process";
import nodeAssert from "node:assert";
```

<a id="ref-q1-12"></a>
### [12] `tests/unit_node/process_test.ts:187-198`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L187-L198)

```typescript
  name: "process.on",
  async fn() {
    assertEquals(typeof process.on, "function");

    let triggered = false;
    process.on("exit", () => {
      triggered = true;
    });
    // @ts-ignore fix the type here
    process.emit("exit");
    assert(triggered);
```

<a id="ref-q1-13"></a>
### [13] `tests/unit_node/process_test.ts:217-267`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L217-L267)

```typescript
  name: "process.on signal",
  ignore: Deno.build.os == "windows",
  async fn() {
    let wait = "";
    const testTimeout = setTimeout(
      () => fail("Test timed out waiting for " + wait),
      10_000,
    );
    try {
      const process = new Deno.Command(Deno.execPath(), {
        args: [
          "eval",
          `
          import process from "node:process";
          setInterval(() => {}, 1000);
          process.on("SIGINT", () => {
            console.log("foo");
          });
          console.log("ready");
          `,
        ],
        stdout: "piped",
        stderr: "null",
      }).spawn();
      let output = "";
      process.stdout.pipeThrough(new TextDecoderStream()).pipeTo(
        new WritableStream({
          write(chunk) {
            console.log("chunk:", chunk);
            output += chunk;
          },
        }),
      );
      wait = "ready";
      while (!output.includes("ready\n")) {
        await delay(10);
      }
      for (let i = 0; i < 3; i++) {
        output = "";
        process.kill("SIGINT");
        wait = "foo " + i;
        while (!output.includes("foo\n")) {
          await delay(10);
        }
      }
      process.kill("SIGTERM");
      await process.status;
    } finally {
      clearTimeout(testTimeout);
    }
  },
```

<a id="ref-q1-14"></a>
### [14] `tests/unit_node/process_test.ts:286-330`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L286-L330)

```typescript
  name: "process.off signal",
  ignore: Deno.build.os == "windows",
  async fn() {
    const testTimeout = setTimeout(() => fail("Test timed out"), 10_000);
    try {
      const process = new Deno.Command(Deno.execPath(), {
        args: [
          "eval",
          `
          import process from "node:process";
          setInterval(() => {}, 1000);
          const listener = () => {
            process.off("SIGINT", listener);
            console.log("foo");
          };
          process.on("SIGINT", listener);
          console.log("ready");
          `,
        ],
        stdout: "piped",
        stderr: "null",
      }).spawn();
      let output = "";
      process.stdout.pipeThrough(new TextDecoderStream()).pipeTo(
        new WritableStream({
          write(chunk) {
            console.log("chunk:", chunk);
            output += chunk;
          },
        }),
      );
      while (!output.includes("ready\n")) {
        await delay(10);
      }
      output = "";
      process.kill("SIGINT");
      while (!output.includes("foo\n")) {
        await delay(10);
      }
      process.kill("SIGINT");
      await process.status;
    } finally {
      clearTimeout(testTimeout);
    }
  },
```

<a id="ref-q1-15"></a>
### [15] `tests/unit_node/process_test.ts:843-864`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L843-L864)

```typescript
Deno.test("process.on, process.off, process.removeListener doesn't throw on unimplemented events", () => {
  const events = [
    "beforeExit",
    "disconnect",
    "message",
    "rejectionHandled",
    "uncaughtException",
    "uncaughtExceptionMonitor",
    "unhandledRejection",
    "worker",
  ];
  const handler = () => {};
  events.forEach((ev) => {
    process.on(ev, handler);
    assertEquals(process.listenerCount(ev), 1);
    process.off(ev, handler);
    assertEquals(process.listenerCount(ev), 0);
    process.on(ev, handler);
    assertEquals(process.listenerCount(ev), 1);
    process.removeListener(ev, handler);
    assertEquals(process.listenerCount(ev), 0);
  });
```

<a id="ref-q1-16"></a>
### [16] `tests/unit_node/process_test.ts:1280-1293`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L1280-L1293)

```typescript
Deno.test("process.listeners - include SIG* events", () => {
  const listener = () => console.log("SIGINT");
  process.on("SIGINT", listener);
  assertEquals(process.listeners("SIGINT").length, 1);

  const listener2 = () => console.log("SIGINT");
  process.prependListener("SIGINT", listener2);
  assertEquals(process.listeners("SIGINT").length, 2);

  process.off("SIGINT", listener);
  assertEquals(process.listeners("SIGINT").length, 1);
  process.off("SIGINT", listener2);
  assertEquals(process.listeners("SIGINT").length, 0);
});
```

<a id="ref-q1-17"></a>
### [17] `tests/unit_node/process_test.ts:1690-1703`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L1690-L1703)

```typescript
Deno.test({
  name: "[node/process] signal handlers receive signal name",
  ignore: Deno.build.os === "windows",
  async fn() {
    const received: string[] = [];
    const handler = (signal: string) => {
      received.push(signal);
    };
    process.once("SIGUSR1", handler);
    process.kill(process.pid, "SIGUSR1");
    // Signal delivery is async; wait for it
    await new Promise((resolve) => setTimeout(resolve, 100));
    assertEquals(received, ["SIGUSR1"]);
  },
```

<a id="ref-q1-18"></a>
### [18] `cli/tsc/dts/node/process.d.cts:1973-1980`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/process.d.cts#L1973-L1980)

```
                /* EventEmitter */
                addListener(event: "beforeExit", listener: BeforeExitListener): this;
                addListener(event: "disconnect", listener: DisconnectListener): this;
                addListener(event: "exit", listener: ExitListener): this;
                addListener(event: "rejectionHandled", listener: RejectionHandledListener): this;
                addListener(event: "uncaughtException", listener: UncaughtExceptionListener): this;
                addListener(event: "uncaughtExceptionMonitor", listener: UncaughtExceptionListener): this;
                addListener(event: "unhandledRejection", listener: UnhandledRejectionListener): this;
```

<a id="ref-q1-19"></a>
### [19] `tests/specs/npm/compare_globals/main.ts:3-7`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/specs/npm/compare_globals/main.ts#L3-L7)

```typescript
import * as globals from "npm:@denotest/globals";
console.log(globals.global === globals.globalThis);
console.log(globals.globalThis === globalThis);
console.log(globals.process.execArgv);
console.log("process equals process", process === globals.process);
```
