# TypeScript/Node.js Parity Gaps

> **⚠️ This document is historical (captured at v0.4.56) and is no longer
> accurate.** Most items listed below as "Missing" or "Partial" — generators and
> the full iterator protocol, `Symbol` / well-known symbols, `Proxy`, `Reflect`,
> async generators, `for await...of`, and more — have since landed. As of
> v0.5.x Perry sits at roughly **95% Node.js/TypeScript compatibility** and
> passes **~97%** of Node's own test suite (node v26, 53 `node:*` modules).
> For the current, maintained gap analysis see
> [`runtime-parity-gaps.md`](runtime-parity-gaps.md); for the language-feature
> status see [`src/language/supported-features.md`](src/language/supported-features.md)
> and [`src/language/limitations.md`](src/language/limitations.md). The rows below
> are kept only for historical reference.

Everything the Perry compiler is missing for absolute parity with Node.js running TypeScript, as of v0.4.56.

**Current status:** 100% pass rate on 87 parity tests (27 edge-case + 60 existing). The gaps below are features NOT covered by those tests — they either aren't tested or aren't implemented.

---

## Language Features

### Generators & Iterators

| Feature | Status | Notes |
|---|---|---|
| `function*` syntax | Parsed | `Expr::Yield` exists in HIR but no generator object protocol |
| `yield` / `yield*` | Partial | Expression exists but `.next()` / `.return()` / `.throw()` not callable on result |
| Generator iterator protocol | Missing | `function*` doesn't produce an object with `.next()` |
| `Symbol.iterator` | Missing | No custom iterable protocol — `for...of` only works on arrays, strings, Maps, Sets |
| `Symbol.asyncIterator` | Missing | No async iterable protocol |
| `async function*` | Missing | Async generators not supported |
| `for await...of` | Missing | Async iteration not supported |

### Symbols

| Feature | Status | Notes |
|---|---|---|
| `Symbol()` constructor | Missing | No runtime Symbol creation |
| `Symbol.iterator` | Missing | Can't make custom iterables |
| `Symbol.toPrimitive` | Missing | Can't customize type coercion |
| `Symbol.hasInstance` | Missing | Can't customize `instanceof` |
| `Symbol.toStringTag` | Missing | Can't customize `Object.prototype.toString` |
| `Symbol.species` | Missing | Can't customize derived constructors |
| Well-known symbols | Missing | None of the `Symbol.*` constants exist |

### Metaprogramming

| Feature | Status | Notes |
|---|---|---|
| `Proxy` | Missing | Explicitly skipped — would require trap checks on every property access |
| `Reflect` | Missing | No reflection API |
| `eval()` | Missing | Fundamentally hard for AOT compiler — would need bundled interpreter |
| `new Function('...')` | Missing | Same constraint as eval |
| `Object.defineProperty()` | Missing | No property descriptor model |
| `Object.getOwnPropertyDescriptor()` | Missing | No property descriptor model |
| `Object.getPrototypeOf()` | Missing | No prototype chain at runtime |
| `Object.setPrototypeOf()` | Missing | No prototype mutation |
| `Object.create()` | Exists | But limited — no property descriptors argument |

### Classes (advanced)

| Feature | Status | Notes |
|---|---|---|
| Private methods `#method()` | Missing | Private fields `#field` work but not methods |
| Static class blocks `static { ... }` | Missing | Static field initializers work but not blocks |
| Class decorators (stage 3) | Partial | Basic decorator syntax parsed, limited runtime support |
| `abstract` classes | Missing | No enforcement — abstract methods not checked |

### Async (advanced)

| Feature | Status | Notes |
|---|---|---|
| `for await...of` | Missing | |
| `Promise.allSettled()` | Missing | |
| `Promise.any()` | Missing | |
| `Promise.race()` | Partial | Runtime function exists (v0.4.47) but may have edge cases |
| Async generators | Missing | `async function*` not supported |
| Async disposal `await using` | Missing | TC39 stage 3 |

### Error Handling (advanced)

| Feature | Status | Notes |
|---|---|---|
| `Error.cause` | Missing | `new Error("msg", { cause: err })` not supported |
| `AggregateError` | Missing | Used by `Promise.any()` |
| `Error.captureStackTrace` | Missing | V8-specific, not in spec |
| Custom error `.stack` formatting | Missing | Stack traces are basic |

### Dynamic Features

| Feature | Status | Notes |
|---|---|---|
| Dynamic `import()` | Broken | Returns undefined with warning |
| `with` statement | Missing | Intentional — deprecated in strict mode |
| Computed property access on unknown types | Partial | Works for known types, falls through for `any` |
| `arguments` object | Missing | Arrow functions don't have it (correct), regular functions should |
| Tagged template literals | Missing | `` tag`string` `` syntax |

---

## Built-in Objects & Methods

### Array

| Method | Status |
|---|---|
| `push`, `pop`, `shift`, `unshift` | ✓ |
| `slice`, `splice` | ✓ |
| `map`, `filter`, `find`, `findIndex` | ✓ |
| `forEach`, `some`, `every` | ✓ |
| `reduce`, `reduceRight` | ✓ reduce only (no reduceRight) |
| `sort` (with and without comparator) | ✓ |
| `reverse` | ✓ |
| `flat`, `flatMap` | ✓ |
| `fill` | ✓ |
| `concat` | ✓ |
| `join` | ✓ |
| `indexOf`, `lastIndexOf` | ✓ |
| `includes` | ✓ |
| `isArray` | ✓ |
| `from`, `from(iter, mapFn)` | ✓ |
| `of` | Missing |
| `at()` | Missing |
| `findLast()` | Missing |
| `findLastIndex()` | Missing |
| `toReversed()` | Missing |
| `toSorted()` | Missing |
| `toSpliced()` | Missing |
| `with()` | Missing |
| `copyWithin()` | Missing |
| `entries()`, `keys()`, `values()` | Missing (as iterators) |
| `Array.fromAsync()` | Missing |
| `group()` / `groupToMap()` | Missing |

### String

| Method | Status |
|---|---|
| `charAt`, `charCodeAt` | ✓ |
| `slice`, `substring`, `substr` | ✓ |
| `trim`, `trimStart`, `trimEnd` | ✓ |
| `toLowerCase`, `toUpperCase` | ✓ |
| `replace`, `replaceAll` | ✓ |
| `split` (string and regex) | ✓ |
| `match`, `matchAll` | ✓ |
| `search` | ✓ |
| `indexOf`, `lastIndexOf` | ✓ |
| `includes`, `startsWith`, `endsWith` | ✓ |
| `padStart`, `padEnd` | ✓ |
| `repeat` | ✓ |
| `fromCharCode` | ✓ |
| `at()` | Missing |
| `normalize()` | Listed but may not work |
| `localeCompare()` | Missing (needs Intl) |
| `toLocaleLowerCase()` / `toLocaleUpperCase()` | Missing (needs Intl) |
| `codePointAt()` | Missing |
| `fromCodePoint()` | Missing |
| `raw()` | Missing |
| `isWellFormed()` / `toWellFormed()` | Missing |

### Object

| Method | Status |
|---|---|
| `keys()`, `values()`, `entries()` | ✓ |
| `assign()` | ✓ |
| `create()` | ✓ (basic) |
| `freeze()`, `seal()`, `preventExtensions()` | No-ops (accepted but don't enforce) |
| `fromEntries()` | Missing |
| `is()` | Missing |
| `hasOwn()` | Missing |
| `defineProperty()` / `defineProperties()` | Missing |
| `getOwnPropertyDescriptor()` / `getOwnPropertyDescriptors()` | Missing |
| `getOwnPropertyNames()` / `getOwnPropertySymbols()` | Missing |
| `getPrototypeOf()` / `setPrototypeOf()` | Missing |
| `isFrozen()` / `isSealed()` / `isExtensible()` | Missing |

### Map & Set

| Method | Status |
|---|---|
| `new Map()`, `new Map(entries)` | ✓ |
| `get`, `set`, `has`, `delete`, `clear` | ✓ |
| `size` | ✓ |
| `forEach` | ✓ |
| `keys()`, `values()`, `entries()` | ✓ (return arrays, not iterators) |
| `new Set()`, `new Set(array)` | ✓ |
| `add`, `has`, `delete`, `clear` | ✓ |
| `size` | ✓ |
| `forEach` | ✓ |
| `values()` | ✓ (returns array) |
| `intersection()`, `union()`, `difference()` | Missing (ES2025) |
| `isSubsetOf()`, `isSupersetOf()`, `isDisjointFrom()` | Missing (ES2025) |
| `symmetricDifference()` | Missing (ES2025) |
| WeakMap | Falls back to Map (no weak references) |
| WeakSet | Falls back to Set (no weak references) |

### RegExp

| Feature | Status |
|---|---|
| Literal patterns `/abc/flags` | ✓ |
| `.test()` | ✓ |
| `.match()` with groups | ✓ |
| `.match()` with global flag | ✓ |
| `.matchAll()` | ✓ |
| `.split(regex)` | ✓ |
| `.search(regex)` | ✓ |
| `.replace(regex, string)` | ✓ |
| `.exec()` | Missing (use `.match()` instead) |
| `lastIndex` property | Missing |
| Named capture groups `(?<name>...)` | Missing |
| Lookbehind assertions `(?<=...)` | Depends on Rust regex crate |
| Unicode property escapes `\p{...}` | Depends on Rust regex crate |
| `d` flag (indices) | Missing |
| `v` flag (unicodeSets) | Missing |

### Number & Math

| Feature | Status |
|---|---|
| All Math methods (floor, ceil, round, abs, sqrt, pow, etc.) | ✓ |
| `Math.trunc`, `Math.sign` | ✓ |
| `Math.random()` | ✓ |
| `Math.clz32()` | Missing |
| `Math.fround()` | Missing |
| `Math.cbrt()` | Missing |
| `Math.expm1()` / `Math.log1p()` | Missing |
| `Math.hypot()` | Missing |
| `Number.MAX_SAFE_INTEGER` etc. | ✓ |
| `Number.isNaN`, `isFinite`, `isInteger`, `isSafeInteger` | ✓ |
| `parseInt`, `parseFloat` | ✓ |
| `toFixed`, `toString(radix)` | ✓ |
| `toPrecision()` | Missing |
| `toExponential()` | Missing |
| Numeric separators `1_000_000` | ✓ (handled by SWC parser) |

### Date

| Feature | Status |
|---|---|
| `new Date()`, `Date.now()` | ✓ |
| `getFullYear/Month/Date/Hours/Minutes/Seconds/Milliseconds` | ✓ |
| `toISOString()` | ✓ |
| `getTime()` | ✓ |
| `setFullYear/Month/Date/Hours/Minutes/Seconds` | Missing |
| `toLocaleDateString()` / `toLocaleTimeString()` | Missing (needs Intl) |
| `toDateString()` / `toTimeString()` | Missing |
| `getTimezoneOffset()` | Missing |
| `toJSON()` | Missing |
| `Date.parse()` | Missing |
| `Date.UTC()` | Missing |

### JSON

| Feature | Status |
|---|---|
| `JSON.parse()` | ✓ |
| `JSON.stringify()` | ✓ |
| `JSON.stringify(value, replacer)` | Missing (replacer function) |
| `JSON.stringify(value, null, space)` | Missing (pretty-print) |
| `JSON.parse(text, reviver)` | Missing (reviver function) |

### Promise

| Feature | Status |
|---|---|
| `new Promise(executor)` | ✓ |
| `.then()`, `.catch()`, `.finally()` | ✓ |
| `Promise.resolve()`, `Promise.reject()` | ✓ |
| `Promise.all()` | ✓ |
| `Promise.race()` | Partial (v0.4.47) |
| `Promise.allSettled()` | Missing |
| `Promise.any()` | Missing |
| `Promise.withResolvers()` | Missing |
| `await` | ✓ |

---

## Global APIs

### Issue #3598 globalThis/Web closure audit

Issue #3598 was closed on 2026-05-31 because the umbrella is superseded by granular child issues. The current `origin/main` audit found no manifest-backed API docs/type delta to regenerate for this docs-only closure branch: `./scripts/regen_api_docs.sh` produced no changes to `docs/src/api/reference.md` or `docs/api/perry.d.ts`.

The tables below reflect the global APIs that are already present on current `main`. Do not mark the #3598 feature-branch globals as complete here until their runtime PRs land; residual child issues should continue tracking weak GC semantics, BroadcastChannel delivery, deeper FormData/File/Blob body parity, full WebAssembly execution classes/streaming, full text encoding stream behavior, and URLPattern behavior for the Node 22 target.

### Timers

| API | Status |
|---|---|
| `setTimeout(fn, ms)` | ✓ |
| `setInterval(fn, ms)` | ✓ |
| `clearTimeout(id)` | ✓ |
| `clearInterval(id)` | ✓ |
| `setImmediate(fn)` | ✓ |
| `clearImmediate(id)` | ✓ |

### Console

| Method | Status |
|---|---|
| `console.log()` | ✓ (with spread) |
| `console.error()` | ✓ |
| `console.warn()` | ✓ |
| `console.table()` | Missing |
| `console.dir()` | ✓ |
| `console.time()` / `timeEnd()` | ✓ |
| `console.timeLog()` | Missing |
| `console.group()` / `groupEnd()` | Missing |
| `console.trace()` | ✓ |
| `console.assert()` | ✓ |
| `console.count()` | ✓ |
| `console.countReset()` | Missing |
| `console.clear()` | Missing |

### Encoding

| API | Status |
|---|---|
| `TextEncoder` | ✓ |
| `TextDecoder` | ✓ |
| `TextEncoderStream` / `TextDecoderStream` | Partial (constructor/global shape; transform behavior remains tracked) |
| `atob()` | ✓ |
| `btoa()` | ✓ |
| `encodeURIComponent()` | ✓ |
| `decodeURIComponent()` | ✓ |
| `encodeURI()` / `decodeURI()` | ✓ |

### Other Globals

| API | Status |
|---|---|
| `structuredClone()` | ✓ (transfer/lifecycle edge cases remain tracked) |
| `queueMicrotask()` | ✓ |
| `AbortController` / `AbortSignal` | Partial |
| `EventTarget` | Partial |
| `performance.now()` | ✓ |
| `navigator` | Partial (Node metadata object; Web Locks is a stub) |
| `URL` / `URLSearchParams` | ✓ |
| `WeakRef` | Partial (strong-reference wrapper; true weak semantics missing) |
| `FinalizationRegistry` | Partial (registration shape; cleanup-on-GC semantics missing) |
| `Intl` (entire namespace) | Missing |

---

## Node.js Standard Library

### fs (File System)

| Function | Status |
|---|---|
| `readFileSync(path)` | ✓ (returns Buffer) |
| `readFileSync(path, encoding)` | ✓ (returns string) |
| `writeFileSync(path, data)` | ✓ |
| `appendFileSync(path, data)` | ✓ |
| `existsSync(path)` | ✓ |
| `mkdirSync(path)` | ✓ |
| `unlinkSync(path)` | ✓ |
| `rmSync(path, { recursive })` | ✓ |
| `statSync()` / `lstatSync()` | Missing |
| `readdirSync()` | Missing |
| `renameSync()` | Missing |
| `copyFileSync()` | Missing |
| `chmodSync()` / `chownSync()` | Missing |
| `watchFile()` / `watch()` | Missing |
| `createReadStream()` / `createWriteStream()` | Missing (needs streams) |
| All async variants (`readFile`, `writeFile`, etc.) | Missing |
| `fs/promises` | Missing |

### path

| Function | Status |
|---|---|
| `join()` | ✓ |
| `dirname()` | ✓ |
| `basename()` | ✓ |
| `extname()` | ✓ |
| `resolve()` | ✓ |
| `isAbsolute()` | ✓ |
| `relative()` | Missing |
| `parse()` | Missing |
| `format()` | Missing |
| `normalize()` | Missing |
| `sep` / `delimiter` | Missing |
| `posix` / `win32` | Missing |

### crypto

| Function | Status |
|---|---|
| `randomBytes(n)` | ✓ |
| `randomUUID()` | ✓ |
| `createHash('sha256')` (via helper) | ✓ (as `sha256()` / `md5()`) |
| `createHash()` (general) | Missing |
| `createHmac()` | Missing |
| `createCipheriv()` / `createDecipheriv()` | Missing |
| `pbkdf2()` / `pbkdf2Sync()` | Missing |
| `scrypt()` / `scryptSync()` | Missing |
| `sign()` / `verify()` | Missing |
| `generateKeyPairSync()` | Missing |
| `subtle` (SubtleCrypto) | Missing |
| `getRandomValues()` | Missing |

### process

| Feature | Status |
|---|---|
| `process.exit(code)` | ✓ |
| `process.env` | ✓ (static and dynamic access) |
| `process.argv` | ✓ |
| `process.cwd()` | ✓ |
| `process.uptime()` | ✓ |
| `process.memoryUsage()` | ✓ |
| `process.platform` | ✓ |
| `process.arch` | ✓ |
| `process.pid` | Missing |
| `process.ppid` | Missing |
| `process.stdin` / `stdout` / `stderr` | Missing (as streams) |
| `process.on('exit', fn)` | Missing |
| `process.on('uncaughtException', fn)` | Missing |
| `process.nextTick()` | Missing |
| `process.hrtime()` / `process.hrtime.bigint()` | Missing |
| `process.version` | Missing |
| `process.versions` | Missing |
| `process.kill()` | Missing |
| `process.chdir()` | Missing |

### child_process

| Function | Status |
|---|---|
| `execSync()` | ✓ |
| `spawnSync()` | ✓ |
| `spawn()` | ✓ (basic) |
| `exec()` | ✓ (basic) |
| Full stdio piping | Missing |
| `.on('data')` event pattern | Missing |
| `.stdin.write()` | Missing |
| `fork()` | Missing |

### os

| Function | Status |
|---|---|
| `platform()`, `arch()`, `hostname()` | ✓ |
| `homedir()`, `tmpdir()` | ✓ |
| `totalmem()`, `freemem()` | ✓ |
| `uptime()`, `type()`, `release()` | ✓ |
| `cpus()` | ✓ |
| `networkInterfaces()` | ✓ |
| `userInfo()` | ✓ |
| `EOL` | ✓ |
| `endianness()` | Missing |
| `loadavg()` | Missing |

### http / https

| Feature | Status |
|---|---|
| `fetch()` global | ✓ (GET/POST/options) |
| `Response.json()` / `.text()` / `.arrayBuffer()` | ✓ |
| `Response.headers` | ✓ |
| `Response.status` / `.statusText` / `.ok` | ✓ |
| `Headers` class | ✓ |
| `Request` class | ✓ |
| `Blob` / `File` | Partial |
| `FormData` | Partial (response/request extraction helpers; constructor and mutators remain tracked) |
| `http.createServer()` | Missing (use Fastify integration instead) |
| `http.request()` / `http.get()` | Missing |

### stream

| Feature | Status |
|---|---|
| `Readable` | Missing |
| `Writable` | Missing |
| `Transform` | Missing |
| `Duplex` | Missing |
| `pipeline()` | Missing |
| `finished()` | Missing |
| Async iteration on streams | Missing |

### events

| Feature | Status |
|---|---|
| `EventEmitter` class | ✓ (basic) |
| `.on()` / `.emit()` | ✓ |
| `.once()` | Missing |
| `.off()` / `.removeListener()` | Missing |
| `.removeAllListeners()` | Missing |
| `.listenerCount()` | Missing |
| `.prependListener()` | Missing |
| `events.once()` (static) | Missing |

### buffer

| Feature | Status |
|---|---|
| `Buffer.from(string, encoding)` | ✓ |
| `Buffer.alloc(size)` | ✓ |
| `Buffer.concat()` | ✓ |
| `Buffer.isBuffer()` | ✓ |
| `.toString(encoding)` | ✓ |
| `.length` | ✓ |
| `.slice()` / `.subarray()` | ✓ |
| `.copy()` | ✓ |
| `.fill()` | ✓ |
| `.equals()` | ✓ |
| Indexed `buf[i]` get/set | ✓ |
| `.readUInt8/16/32()` etc. | Missing |
| `.writeUInt8/16/32()` etc. | Missing |
| `.readBigInt64()` etc. | Missing |
| `.compare()` | Missing |
| `.swap16/32/64()` | Missing |
| `.indexOf()` / `.includes()` | Missing |

### Modules not implemented at all

| Module | Notes |
|---|---|
| `util` | `promisify`, `inspect`, `types`, `format` |
| `assert` | Testing assertions |
| `zlib` | Compression |
| `dns` | DNS resolution |
| `tls` | TLS/SSL |
| `cluster` | Multi-process |
| `worker_threads` | Worker threads (Perry has its own `perry/thread`) |
| `readline` | Interactive input |
| `querystring` | URL query parsing (use `URLSearchParams` instead) |
| `timers/promises` | Timer promises |
| `perf_hooks` | Performance measurement |
| `async_hooks` | Partial (AsyncLocalStorage exists) |
| `diagnostics_channel` | Diagnostics |
| `inspector` | V8 inspector |
| `vm` | Virtual machine contexts |
| `string_decoder` | String decoding |
| `punycode` | Unicode encoding |
| `domain` | Error domains (deprecated) |
| `v8` | V8 engine API |
| `trace_events` | Tracing |

---

## Architecture Constraints

These are features that are fundamentally difficult for an AOT native compiler:

1. **`eval()` / `new Function()`** — requires a runtime interpreter. Perry has QuickJS fallback but it's opt-in.
2. **`Proxy`/`Reflect`** — every property access would need a trap check. Devastating for performance.
3. **Full `Symbol.iterator` protocol** — requires all `for...of` loops to go through a `.next()` call chain instead of direct indexed access.
4. **Dynamic `import()`** — requires lazy compilation or pre-bundling. Perry resolves all imports at compile time.
5. **`with` statement** — deprecated in strict mode, would break all scope resolution optimizations.
6. **Full prototype chain** — Perry uses flat field arrays, not prototype chains. `Object.getPrototypeOf()` etc. are impossible without restructuring object layout.
7. **Full `Intl`** — requires ICU data (~27MB) or system ICU linkage.
8. **`SharedArrayBuffer`/`Atomics`** — Perry uses deep-copy semantics across threads. Shared mutable memory would require a different threading model.

---

## Summary Statistics

| Category | Implemented | Missing | Coverage |
|---|---|---|---|
| Core language syntax | 48/52 | 4 | 92% |
| Built-in object methods | ~120/180 | ~60 | 67% |
| Node.js std lib functions | ~65/200+ | ~135+ | ~30% |
| Global APIs | ~24/30 | ~6 | 80% |
| **Overall language parity** | | | **~85%** |
| **Overall Node.js API parity** | | | **~35%** |

The language itself is nearly complete. The gap is primarily in the Node.js standard library, the `Intl` namespace, advanced async patterns (generators, async iterators), and metaprogramming (`Symbol`, `Proxy`, `Reflect`).
