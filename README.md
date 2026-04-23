# Perry

**One codebase. Every platform. Native performance.**

Perry is a native TypeScript compiler written in Rust. It takes your TypeScript and compiles it straight to native executables — no Node.js, no Electron, no browser engine. Just fast, small binaries that run anywhere.

**Current Version:** 0.5.152 | [Website](https://perryts.com) | [Documentation](https://perryts.github.io/perry/) | [Showcase](https://perryts.com/showcase)

```bash
perry compile src/main.ts -o myapp
./myapp    # that's it — a standalone native binary
```

Perry uses [SWC](https://swc.rs/) for TypeScript parsing and [LLVM](https://llvm.org/) for native code generation. The output is a single binary with no runtime dependencies.

---

## Built with Perry

People are building real apps with Perry today. Here are some highlights:

| Project | What it is | Platforms |
|---------|-----------|-----------|
| [**Bloom Engine**](https://bloomengine.dev) | Native TypeScript game engine — Metal, DirectX 12, Vulkan, OpenGL. Write games in TS, ship native. | macOS, Windows, Linux, iOS, tvOS, Android |
| [**Mango**](https://github.com/MangoQuery/app) | Native MongoDB GUI. ~7 MB binary, <100 MB RAM, sub-second cold start. | macOS, Windows, Linux, iOS, Android |
| [**Hone**](https://hone.codes) | AI-powered native code editor with built-in terminal, Git, and LSP. | macOS, Windows, Linux, iOS, Android, Web |
| [**Pry**](https://github.com/nicktrebes/perry-pry) | Fast, native JSON viewer with tree navigation and search. | macOS, iOS, Android |
| [**dB Meter**](https://dbmeter.app) | Real-time sound level measurement with 60fps updates and per-device calibration. | iOS, macOS, Android |

### Screenshots

**Mango** — Native MongoDB GUI ([source](https://github.com/MangoQuery/app))

<p align="center">
  <img src="docs/images/showcase/mango-explorer.png" width="720" alt="Mango — database explorer view" />
</p>
<p align="center">
  <img src="docs/images/showcase/mango-editor.png" width="720" alt="Mango — document editor view" />
</p>

**Hone** — AI-powered native code editor ([hone.codes](https://hone.codes))

<p align="center">
  <img src="https://hone.codes/screenshot.png" width="720" alt="Hone — AI code editor built with Perry" />
</p>

> Have something you've built with Perry? Open a PR to add it here!

---

## Performance

Perry beats Node.js and Bun on every benchmark below. Best of 5 runs (best of 3 for cheap cells), macOS ARM64 (Apple Silicon), Node.js v25, Bun 1.3, rerun 2026-04-23 on v0.5.173.

| Benchmark | Perry | Node.js | Bun | vs Node | What it tests |
|-----------|-------|---------|-----|---------|---------------|
| factorial | 31ms | 596ms | 98ms | **19x faster** | Modular accumulation (integer fast path) |
| method_calls | 1ms | 11ms | 9ms | **11x faster** | Class method dispatch (10M calls) |
| loop_overhead | 15ms | 61ms | 50ms | **4x faster** | Tight numeric loop (100M iterations) |
| math_intensive | 14ms | 52ms | 52ms | **3.7x faster** | Harmonic series (50M iterations) |
| fibonacci(40) | 320ms | 1033ms | 521ms | **3.2x faster** | Recursive function calls (i64 specialization) |
| array_read | 5ms | 13ms | 15ms | **2.6x faster** | Sequential read (10M elements) |
| closure | 10ms | 309ms | 51ms | **31x faster** | Closure creation + invocation (10M calls) |
| array_write | 4ms | 9ms | 7ms | **2.3x faster** | Sequential write (10M elements) |
| object_create | 3ms | 9ms | 8ms | **3x faster** | Object allocation (1M objects, scalar replacement) |
| binary_trees | 3ms | 10ms | 7ms | **3.3x faster** | Tree allocation + traversal (1M nodes, scalar replacement) |
| string_concat | 0ms | 3ms | 2ms | **fast** | 100K string appends |
| nested_loops | 9ms | 20ms | 20ms | **2.2x faster** | Nested array access (3000x3000) |
| prime_sieve | 5ms | 8ms | 7ms | **1.6x faster** | Sieve of Eratosthenes |
| mandelbrot | 23ms | 25ms | 30ms | **1.1x faster** | Complex f64 iteration (800x800) |
| matrix_multiply | 24ms | 34ms | 35ms | **1.4x faster** | 256x256 matrix multiply |
| json_roundtrip | 314ms | 377ms | 250ms | **1.2x faster** | 50× `JSON.parse` + `JSON.stringify` on a ~1MB, 10K-item blob |

Perry compiles to native machine code via LLVM — no JIT warmup, no interpreter overhead. Key optimizations: **scalar replacement** of non-escaping objects (escape analysis eliminates heap allocation entirely — object fields become registers), inline bump allocator for objects that do escape, i32 loop counters for bounded array access, `reassoc contract` fast-math flags, integer-modulo fast path (`fptosi → srem → sitofp` instead of `fmod`), elimination of redundant `js_number_coerce` calls on numeric function returns, i64 specialization for pure numeric recursive functions, and `<2 x double>` parallel-accumulator vectorization on pure-fadd reduction loops (restored in v0.5.164 via [#140](https://github.com/PerryTS/perry/issues/140)).

### Perry vs compiled languages

Perry also competes with systems languages. All implementations use `f64`/`double` to match TypeScript's `number` type — no SIMD intrinsics, no unsafe code. See [`benchmarks/polyglot/`](benchmarks/polyglot/) for source and methodology.

| Benchmark | Perry | Rust | C++ | Go | Swift | Java | Node | Bun | Python |
|-----------|-------|------|-----|----|-------|------|------|-----|--------|
| fibonacci | 309 | 321 | **308** | 445 | 410 | **280** | 1001 | 516 | 16055 |
| loop_overhead | **12** | 96 | 96 | 96 | 96 | 98 | 58 | 40 | 3011 |
| array_write | 3 | 7 | **2** | 8 | **2** | 6 | 9 | 6 | 396 |
| array_read | **3** | 9 | 9 | 10 | 9 | 11 | 13 | 16 | 344 |
| math_intensive | **14** | 48 | 50 | 49 | 48 | 50 | 49 | 50 | 2245 |
| object_create | 2 | **0** | **0** | **0** | **0** | 5 | 8 | 6 | 160 |
| nested_loops | 10 | **8** | **8** | 9 | **8** | 10 | 17 | 20 | 476 |
| accumulate | **25** | 95 | 97 | 97 | 97 | 97 | 594 | 99 | 4991 |

Perry leads on 5 of 8 cells (`loop_overhead`, `array_read`, `math_intensive`, `accumulate`, and tied on `fibonacci` within noise), trails by 1-2 ms on three (`object_create`, `nested_loops`, `array_write`) where Rust/C++/Go/Swift benefit from stack-allocated struct layout. Perry's biggest wins — 8× on `loop_overhead`, 3-4× on `math_intensive` / `accumulate` — come from `<2 x double>` parallel-accumulator autovectorization of pure-fadd reduction loops, restored in v0.5.164 after the regression tracked as [#140](https://github.com/PerryTS/perry/issues/140).

**The `loop_overhead` / `math_intensive` / `accumulate` gaps vs Rust/C++ come from two stacked optimizations, not a codegen-backend advantage.** (1) Perry emits `reassoc contract` on f64 ops because TS `number` semantics can't observe the difference; Rust/C++/Go/Swift default to strict-IEEE fadd (3-cycle latency wall, unreassociable). (2) On top of `reassoc`, LLVM autovectorizes the body into a `<2 x double>` parallel-accumulator reduction with interleave count 4. Same `bench.cpp` with `g++ -O3 -ffast-math` hits 11 ms on `loop_overhead` — same LLVM, same pipeline, one flag flip. Go has no fast-math flag at all, which is why it matches Rust/C++ despite using a different backend. [`benchmarks/polyglot/RESULTS_OPT.md`](benchmarks/polyglot/RESULTS_OPT.md) documents the per-language opt sweep: with fast-math enabled, C++ matches Perry to the millisecond. `object_create` stays at 0ms thanks to scalar replacement — non-escaping objects are decomposed into register-allocated fields, matching the 0ms floor Rust/C++ get from stack allocation. See [`benchmarks/polyglot/RESULTS.md`](benchmarks/polyglot/RESULTS.md) for the full writeup.

### LLVM backend progress

Perry switched from Cranelift to LLVM as its sole code generation backend in v0.5.0. The initial cutover had significant performance regressions due to NaN-boxing overhead in the new backend. Subsequent optimization work recovered and surpassed the original numbers:

| Benchmark | Cranelift | LLVM v0.5.0 | LLVM now | Node.js |
|-----------|-----------|-------------|----------|---------|
| method_calls | 16ms | 1,084ms | **1ms** | 11ms |
| math_intensive | 370ms | 131ms | **14ms** | 52ms |
| object_create | 5ms | 318ms | **3ms** | 9ms |
| binary_trees | — | — | **3ms** | 10ms |
| matrix_multiply | 61ms | 184ms | **24ms** | 34ms |
| nested_loops | 32ms | 57ms | **9ms** | 20ms |
| array_read | 4ms | 26ms | **5ms** | 13ms |
| mandelbrot | 71ms | 47ms | **23ms** | 25ms |
| string_concat | 7ms | 0–1ms | **0ms** | 3ms |
| prime_sieve | 11ms | 11ms | **5ms** | 8ms |
| fibonacci(40) | 505ms | 1,156ms | **320ms** | 1033ms |
| closure | — | — | **10ms** | 309ms |
| factorial | — | — | **31ms** | 596ms |
| json_roundtrip | — | — | **314ms** | 377ms |

The Cranelift column is from the pre-v0.5.0 era (the old README on `main`). LLVM v0.5.0 was the initial cutover — it regressed badly because the new backend routed most operations through runtime helpers instead of inlining them. The current LLVM column shows the state after scalar replacement of non-escaping objects, inline bump allocators, i32 loop counters, fast-math flags, integer-mod fast paths, loop-invariant length hoisting, and redundant number-coerce elimination. LLVM now beats both Cranelift and Node on every workload.

### A note on compile times

Cranelift is often praised for fast compilation, and it is — but the difference is smaller than you'd expect. Perry previously used Cranelift and switched to LLVM in v0.5.0. Compile times increased by only ~20-50ms (8-19%), because the bulk of Perry's compile time is SWC parsing, HIR lowering, and linking — not the codegen backend. On a typical file LLVM adds about 25ms over Cranelift while producing code that runs up to 24x faster. A worthwhile trade.

Run benchmarks yourself: `cd benchmarks/suite && ./run_benchmarks.sh` (requires node, cargo; optional: bun, shermes).

## Binary Size

Perry produces small, self-contained binaries with no external dependencies at run time:

| Program | Binary Size |
|---------|-------------|
| `console.log("Hello, world!")` | **~330KB** |
| hello world + `fs` / `path` / `process` imports | ~380KB |
| full stdlib app (fastify, mysql2, etc.) | ~48MB |
| with `--enable-js-runtime` (V8 embedded) | +~15MB |

Perry automatically detects which parts of the runtime your program uses and only links what's needed.

---

## Installation

### npm / npx (any platform)

Perry ships as a prebuilt-binary npm package — the fastest way to try it, and the only install path that works on all seven supported platforms (macOS arm64/x64, Linux x64/arm64 glibc + musl, Windows x64) with one command:

```bash
# Project-local (recommended — pins Perry's version alongside your deps)
npm install @perryts/perry
npx perry compile src/main.ts -o myapp && ./myapp

# Global
npm install -g @perryts/perry
perry compile src/main.ts -o myapp

# Zero-install, one-shot
npx -y @perryts/perry compile src/main.ts -o myapp
```

[`@perryts/perry`](https://www.npmjs.com/package/@perryts/perry) is a thin launcher; npm automatically picks the matching prebuilt via `optionalDependencies` (`@perryts/perry-darwin-arm64`, `@perryts/perry-linux-x64-musl`, etc.) based on your `os` / `cpu` / `libc`. Requires Node.js ≥ 16 and a system C toolchain for linking (same as any Perry install — see [Requirements](#requirements)).

### macOS (Homebrew)

```bash
brew install perryts/perry/perry
```

### Windows (winget)

```bash
winget install PerryTS.Perry
```

### Debian / Ubuntu (APT)

```bash
curl -fsSL https://perryts.github.io/perry-apt/perry.gpg.pub | sudo gpg --dearmor -o /usr/share/keyrings/perry.gpg
echo "deb [signed-by=/usr/share/keyrings/perry.gpg] https://perryts.github.io/perry-apt stable main" | sudo tee /etc/apt/sources.list.d/perry.list
sudo apt update && sudo apt install perry
```

### Quick install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/PerryTS/perry/main/packaging/install.sh | sh
```

### From source

```bash
git clone https://github.com/PerryTS/perry.git
cd perry
cargo build --release
# Binary at: target/release/perry
```

### Requirements

Perry requires a C linker to link compiled executables:
- **macOS:** Xcode Command Line Tools (`xcode-select --install`)
- **Linux:** GCC or Clang (`sudo apt install build-essential`)
- **Windows:** MSVC (Visual Studio Build Tools)

Run `perry doctor` to verify your environment.

---

## Quick Start

```bash
# Initialize a new project
perry init my-project
cd my-project

# Compile and run
perry compile src/main.ts -o myapp
./myapp

# Or compile and run in one step
perry run .

# Check TypeScript compatibility
perry check src/

# Diagnose environment
perry doctor
```

---

## Real-World Example: API Server with ESM Modules

Perry supports standard ES module imports and npm packages. Here's a real-world API server with multi-file project structure:

**Project layout:**
```
my-api/
├── package.json
├── src/
│   ├── main.ts
│   ├── config.ts
│   └── routes/
│       └── users.ts
└── node_modules/
```

**src/config.ts**
```typescript
export const config = {
  port: 3000,
  dbHost: process.env.DB_HOST || 'localhost',
};
```

**src/routes/users.ts**
```typescript
export function getUsers(): object[] {
  return [
    { id: 1, name: 'Alice' },
    { id: 2, name: 'Bob' },
  ];
}

export function getUserById(id: number): object | undefined {
  return getUsers().find((u: any) => u.id === id);
}
```

**src/main.ts**
```typescript
import fastify from 'fastify';
import { config } from './config';
import { getUsers, getUserById } from './routes/users';

const app = fastify();

app.get('/api/users', async () => {
  return getUsers();
});

app.get('/api/users/:id', async (request) => {
  const { id } = request.params as { id: string };
  return getUserById(parseInt(id));
});

app.listen({ port: config.port }, () => {
  console.log(`Server running on port ${config.port}`);
});
```

**Compile and run:**
```bash
perry compile src/main.ts -o my-api && ./my-api
# or: perry run .
```

The output is a standalone binary — no `node_modules` needed at runtime.

---

## Example Projects

The `example-code/` directory contains ready-to-run projects showing Perry in real-world scenarios:

| Example | Stack | What it demonstrates |
|---------|-------|---------------------|
| **[express-postgres](example-code/express-postgres/)** | Express + PostgreSQL | Multi-file routes, middleware (CORS, Helmet), connection pooling, error handling |
| **[fastify-redis-mysql](example-code/fastify-redis-mysql/)** | Fastify + Redis + MySQL | Rate limiting, caching layer, database queries, dotenv config |
| **[hono-mongodb](example-code/hono-mongodb/)** | Hono + MongoDB | Lightweight HTTP framework with document database |
| **[nestjs-typeorm](example-code/nestjs-typeorm/)** | NestJS + TypeORM | Decorator-based architecture, dependency injection |
| **[nextjs-prisma](example-code/nextjs-prisma/)** | Next.js-style + Prisma | ORM integration, database migrations |
| **[koa-redis](example-code/koa-redis/)** | Koa + Redis | Middleware composition, session storage |
| **[http-server](example-code/http-server/)** | Raw HTTP | Low-level request handling, routing, JSON APIs |
| **[blockchain-demo](example-code/blockchain-demo/)** | Custom | Blockchain implementation in pure TypeScript |

Each example has its own `package.json` and can be compiled with:

```bash
cd example-code/fastify-redis-mysql
npm install
perry compile src/index.ts -o server && ./server
```

---

## Native UI

Perry includes a declarative UI system (`perry/ui`) that compiles directly to native platform widgets — no WebView, no Electron. The programming model is SwiftUI-like: compose native widgets with stack-based layout, alignment, and distribution — not CSS/HTML.

```typescript
import {
  App, VStack, HStack, Text, Button, Spacer, SplitView, splitViewAddChild,
  stackSetAlignment, stackSetDistribution, widgetAddChild, widgetMatchParentWidth,
} from 'perry/ui';

// Sidebar + content layout with a split view
const sidebar = VStack(8, [Text("Projects"), Text("Settings"), Spacer()]);
sidebar.setEdgeInsets(12, 12, 12, 12);
sidebar.setBackgroundColor("#F5F5F5");

const header = HStack(8, [Text("Dashboard"), Spacer(), Button("New", () => {})]);
const actions = HStack(8, [Button("Cancel", () => {}), Button("Save", () => {})]);
stackSetDistribution(actions, 1); // FillEqually — both buttons get equal width

const content = VStack(16, [header, Text("Welcome back!"), Spacer(), actions]);
content.setEdgeInsets(20, 20, 20, 20);
stackSetAlignment(content, 5); // Leading — children align left

const split = SplitView();
splitViewAddChild(split, sidebar);
splitViewAddChild(split, content);

App({ title: 'My App', width: 800, height: 500, body: split });
```

**10 target outputs from one codebase:**

| Platform | Backend | Target Flag |
|----------|---------|-------------|
| macOS | AppKit (NSView) | *(default on macOS)* |
| iOS / iPadOS | UIKit | `--target ios` / `--target ios-simulator` |
| visionOS | UIKit (2D windows) | `--target visionos` / `--target visionos-simulator` |
| tvOS | UIKit | `--target tvos` / `--target tvos-simulator` |
| watchOS | WatchKit | `--target watchos` / `--target watchos-simulator` |
| Android | Android Views (JNI) | `--target android` |
| Windows | Win32 | *(default on Windows)* |
| Linux | GTK4 | *(default on Linux)* |
| Web | DOM (JS codegen) | `--target web` |
| WebAssembly | DOM (WASM) | `--target wasm` |

**127+ UI functions** — widgets (Button, Text, TextField, Toggle, Slider, Picker, Table, Canvas, Image, ProgressView, SecureField, NavigationStack, ZStack, LazyVStack, Form/Section, CameraView, SplitView), layout control (alignment, distribution, match-parent, content hugging, overlay positioning, edge insets), and system APIs (keychain, notifications, file dialogs, clipboard, dark mode, openURL, audio capture).

---

## Multi-Threading

The `perry/thread` module provides real OS threads with compile-time safety — no shared mutable state, no data races:

```typescript
import { parallelMap, parallelFilter, spawn } from 'perry/thread';

// Data-parallel array processing across all CPU cores
const results = parallelMap([1, 2, 3, 4, 5], n => fibonacci(n));

// Parallel filtering
const evens = parallelFilter(numbers, n => n % 2 === 0);

// Background thread with Promise
const result = await spawn(() => expensiveComputation());
```

Values cross threads via deep-copy. Each thread gets its own arena and GC. The compiler enforces that closures don't capture mutable state.

---

## Internationalization (i18n)

Compile-time localization with zero runtime overhead:

```typescript
import { t, Currency, ShortDate } from 'perry/i18n';

console.log(t('hello'));                    // "Hallo" (German locale)
console.log(t('items', { count: 3 }));     // "3 Artikel" (CLDR plural rules)
console.log(Currency(9.99, 'EUR'));         // "9,99 €"
console.log(ShortDate(Date.now()));        // "24.03.2026"
```

Configure in `perry.toml`:

```toml
[i18n]
default_locale = "en"
locales = ["en", "de", "fr", "ja"]
```

All locale strings are baked into the binary at compile time. Native locale detection on all 6 platforms. CLDR plural rules for 30+ locales.

---

## Home Screen Widgets (WidgetKit)

Build native home screen widgets from TypeScript — iOS, Android, watchOS, and Wear OS:

```bash
perry compile src/widget.ts --target ios-widget -o MyWidget
perry compile src/widget.ts --target android-widget -o MyWidget
perry compile src/widget.ts --target watchos-widget -o MyWidget
perry compile src/widget.ts --target wearos-tile -o MyWidget
```

---

## Cross-Platform Targets

```bash
# Desktop (default for host platform)
perry compile src/main.ts -o myapp

# Mobile
perry compile src/main.ts --target ios -o MyApp
perry compile src/main.ts --target ios-simulator -o MyApp
perry compile src/main.ts --target visionos -o MyApp
perry compile src/main.ts --target visionos-simulator -o MyApp
perry compile src/main.ts --target android -o MyApp

# TV / Watch
perry compile src/main.ts --target tvos -o MyApp
perry compile src/main.ts --target watchos -o MyApp

# Web
perry compile src/main.ts --target web -o app.html       # JavaScript output
perry compile src/main.ts --target wasm -o app.wasm      # WebAssembly output

# Home screen widgets
perry compile src/widget.ts --target ios-widget -o MyWidget
perry compile src/widget.ts --target android-widget -o MyWidget
perry compile src/widget.ts --target wearos-tile -o MyWidget
```

---

## Publishing

```bash
perry publish macos   # or: ios / android / linux
```

`perry publish` sends your TypeScript source to perry-hub (the cloud build server), which cross-compiles and signs for each target platform.

---

## Supported Language Features

### Core TypeScript

| Feature | Status |
|---------|--------|
| Variables (let, const, var) | ✅ |
| All operators (+, -, *, /, %, **, &, \|, ^, <<, >>, ???, ?., ternary) | ✅ |
| Control flow (if/else, for, while, switch, break, continue) | ✅ |
| Try-catch-finally, throw | ✅ |
| Functions, arrow functions, rest params, defaults | ✅ |
| Closures with mutable captures | ✅ |
| Classes (inheritance, private fields #, static, getters/setters, super) | ✅ |
| Generics (monomorphized at compile time) | ✅ |
| Interfaces, type aliases, union types, type guards | ✅ |
| Async/await, Promise | ✅ |
| Generators (function*) | ✅ |
| ES modules (import/export, re-exports, `import * as`) | ✅ |
| Destructuring (array, object, rest, defaults, rename) | ✅ |
| Spread operator in calls and literals | ✅ |
| RegExp (test, match, replace) | ✅ |
| BigInt (256-bit) | ✅ |
| Decorators | ❌ ([not supported](docs/src/language/limitations.md#no-decorators)) |

### Standard Library

| Module | Functions |
|--------|-----------|
| `console` | log, error, warn, debug |
| `fs` | readFileSync, writeFileSync, existsSync, mkdirSync, unlinkSync, readdirSync, statSync, readFileBuffer, rmRecursive |
| `path` | join, dirname, basename, extname, resolve |
| `process` | env, exit, cwd, argv, uptime, memoryUsage |
| `JSON` | parse, stringify |
| `Math` | floor, ceil, round, abs, sqrt, pow, min, max, random, log, sin, cos, tan, PI |
| `Date` | Date.now(), new Date(), toISOString(), component getters |
| `crypto` | randomBytes, randomUUID, sha256, md5 |
| `os` | platform, arch, hostname, homedir, tmpdir, totalmem, freemem, uptime, type, release |
| `Buffer` | from, alloc, allocUnsafe, byteLength, isBuffer, concat; instance methods |
| `child_process` | execSync, spawnSync, spawnBackground, getProcessStatus, killProcess |
| `Map` | get, set, has, delete, size, clear, forEach, keys, values, entries |
| `Set` | add, has, delete, size, clear, forEach |
| `setTimeout/clearTimeout` | ✅ |
| `setInterval/clearInterval` | ✅ |
| `worker_threads` | parentPort, workerData |

### Native npm Package Implementations

These packages are natively implemented in Rust — no Node.js required:

| Category | Packages |
|----------|----------|
| **HTTP** | fastify, axios, node-fetch, ws (WebSocket) |
| **Database** | mysql2, pg, ioredis |
| **Security** | bcrypt, argon2, jsonwebtoken |
| **Utilities** | dotenv, uuid, nodemailer, zlib, node-cron |

---

## Compiling npm Packages Natively

Perry can compile pure TypeScript/JavaScript npm packages directly to native code instead of routing them through the V8 runtime. Add a `perry.compilePackages` array to your `package.json`:

```json
{
  "perry": {
    "compilePackages": [
      "@noble/curves",
      "@noble/hashes",
      "superstruct"
    ]
  }
}
```

Then compile with `--enable-js-runtime` as usual. Packages in the list are compiled natively; all others use the V8 runtime.

**Good candidates:** Pure math/crypto libraries, serialization/encoding, data structures with no I/O.
**Keep as V8-interpreted:** Packages using HTTP/WebSocket, native addons, or unsupported Node.js builtins.

---

## Compiler Optimizations

- **Scalar Replacement** — escape analysis identifies non-escaping objects (`let p = new Point(x, y); sum += p.x + p.y`); fields are decomposed into stack allocas that LLVM promotes to registers — zero heap allocation
- **NaN-Boxing** — all values are 64-bit words (f64/u64); no boxing overhead for numbers
- **Mark-Sweep GC** — conservative stack scan, arena block walking, 8-byte GcHeader per alloc
- **Inline Bump Allocator** — objects that do escape use a 13-cycle inline arena bump (no function call on hot path)
- **Parallel Compilation** — rayon-based module codegen, transform passes, and symbol scanning across CPU cores
- **FMA / CSE / Loop Unrolling** — fused multiply-add, common subexpression elimination, 8x loop unroll
- **Fast-Math Flags** — `reassoc contract` on all f64 ops enables LLVM to break serial accumulator chains into parallel accumulators + NEON vectorization
- **Integer-Modulo Fast Path** — `fptosi → srem → sitofp` instead of `fmod` for provably-integer locals (64x speedup on factorial)
- **i64 Specialization** — pure numeric recursive functions compile to native `i64` registers (no f64 round-trips)
- **i32 Loop Counters** — integer registers for loop variables (no f64 round-trips)
- **LICM** — loop-invariant code motion for nested loops
- **Shape-Cached Objects** — 5-6x faster object allocation for escaping objects
- **TimSort** — O(n log n) hybrid sort for `Array.sort()`
- **`__platform__` Constant** — compile-time platform elimination (dead code removal per target)

---

## Plugin System

Compile TypeScript as a native shared library plugin:

```bash
perry compile my-plugin.ts --output-type dylib -o my-plugin.dylib
```

```typescript
import { PluginRegistry } from 'perry/plugin';

export function activate(api: any) {
  api.registerTool('my-tool', (args: any) => { /* ... */ });
  api.on('event', (data: any) => { /* ... */ });
}
```

---

## Testing (Geisterhand)

Perry includes Geisterhand, an in-process UI testing framework with HTTP-driven interaction and screenshot capture:

```bash
perry compile src/main.ts --enable-geisterhand -o myapp
./myapp
# UI test server runs on http://localhost:7676
```

Supports screenshot capture on all native platforms. See the [Geisterhand docs](https://perryts.github.io/perry/testing/geisterhand.html) for details.

---

## Ecosystem

| Package | Description |
|---------|-------------|
| [**Bloom Engine**](https://bloomengine.dev) | Native TypeScript game engine — 2D/3D rendering, skeletal animation, spatial audio, physics. Metal/DirectX 12/Vulkan/OpenGL. |
| [perry-react](https://github.com/PerryTS/react) | React/JSX that compiles to native widgets. Standard React components → native macOS/iOS/Android app. |
| [perry-sqlite](https://github.com/PerryTS/sqlite) | SQLite with a Prisma-compatible API (`findMany`, `create`, `upsert`, `$transaction`, etc.) |
| [perry-postgres](https://github.com/PerryTS/postgres) | PostgreSQL with the same Prisma-compatible API |
| [perry-prisma](https://github.com/PerryTS/prisma) | MySQL with the same Prisma-compatible API |
| [perry-apn](https://github.com/PerryTS/push) | Apple Push Notifications (APNs) native library |
| [@perryts/threads](https://github.com/PerryTS/perry/tree/main/packages/perry-threads) | Web Worker parallelism (`parallelMap`, `parallelFilter`, `spawn`) for browser/Node.js |
| [perry-starter](https://github.com/PerryTS/starter) | Minimal starter project — get up and running in 30 seconds |
| [perry-demo](https://demo.perryts.com) | Live benchmark dashboard comparing Perry vs Node.js vs Bun |
| [perry-react-dom](https://github.com/PerryTS/react-dom) | Perry React DOM bridge |

### perry-react

Write React components that compile to native widgets — no DOM, no browser:

```tsx
import { useState } from 'react';
import { createRoot } from 'react-dom/client';

function Counter() {
  const [n, setN] = useState(0);
  return (
    <div>
      <h1>Count: {n}</h1>
      <button onClick={() => setN(n + 1)}>+</button>
    </div>
  );
}

createRoot(null, { title: 'Counter', width: 300, height: 200 }).render(<Counter />);
```

### perry-sqlite / perry-postgres / perry-prisma

Drop-in replacements for `@prisma/client` backed by Rust (sqlx):

```typescript
import { PrismaClient } from 'perry-sqlite';

const prisma = new PrismaClient();
await prisma.$connect();

const users = await prisma.user.findMany({
  where: { email: { contains: '@example.com' } },
  orderBy: { createdAt: 'desc' },
  take: 20,
});

await prisma.$disconnect();
```

---

## Commands

| Command | What it does |
|---------|-------------|
| `perry compile <input.ts> -o <output>` | Compile TypeScript to a native binary |
| `perry run <path> [platform]` | Compile and run in one step (supports `ios`, `android`, etc.) |
| `perry init <name>` | Scaffold a new project |
| `perry check <path>` | Validate TypeScript compatibility without compiling |
| `perry publish <platform>` | Build, sign, and publish via the cloud build server |
| `perry doctor` | Check your development environment |
| `perry i18n extract` | Extract translatable strings from source |

### Compiler flags

```
-o, --output <name>      Output file name
--target <target>        ios | ios-simulator | visionos | visionos-simulator |
                         tvos | tvos-simulator | watchos | watchos-simulator | android |
                         web | wasm | ios-widget | android-widget |
                         wearos-tile | watchos-widget
--output-type <type>     executable | dylib
--enable-js-runtime      Embed V8 for npm package compatibility (+~15MB)
--enable-geisterhand     Enable UI testing server
--print-hir              Print HIR for debugging
```

---

## Project Structure

```
perry/
├── crates/
│   ├── perry/                  # CLI (compile, run, check, init, doctor, publish)
│   ├── perry-parser/           # SWC TypeScript parser
│   ├── perry-types/            # Type system
│   ├── perry-hir/              # HIR data structures and AST→HIR lowering
│   ├── perry-transform/        # IR passes (closure conversion, async, inlining)
│   ├── perry-codegen/          # LLVM native codegen
│   ├── perry-codegen-js/       # JavaScript codegen (--target web)
│   ├── perry-codegen-wasm/     # WebAssembly codegen (--target wasm)
│   ├── perry-codegen-swiftui/  # SwiftUI codegen (iOS/watchOS widgets)
│   ├── perry-codegen-glance/   # Android Glance widget codegen
│   ├── perry-codegen-wear-tiles/ # Wear OS Tiles codegen
│   ├── perry-runtime/          # Runtime (NaN-boxing, GC, arena, strings)
│   ├── perry-stdlib/           # Node.js API support (fastify, mysql2, redis, etc.)
│   ├── perry-ui-*/             # Native UI (macOS, iOS, tvOS, watchOS, Android, GTK4, Windows)
│   ├── perry-ui-geisterhand/   # UI testing framework
│   ├── perry-jsruntime/        # Optional V8 interop via QuickJS
│   └── perry-diagnostics/      # Error reporting
├── docs/                       # Documentation site (mdBook)
├── example-code/               # 8 example applications
├── benchmarks/                 # Benchmark suite (Perry vs Node.js vs Bun)
├── packages/                   # npm packages (@perryts/threads)
└── test-files/                 # Test suite
```

---

## Runtime Characteristics

- **Garbage Collection** — mark-sweep GC with conservative stack scanning, arena block walking, 8-byte GcHeader per allocation
- **Single-Threaded by Default** — async I/O on Tokio workers, callbacks on main thread. Use `perry/thread` for explicit multi-threading.
- **No Runtime Type Checking** — types erased at compile time. Use `typeof` and `instanceof` for runtime checks.
- **Small Binaries** — ~330KB hello world, ~48MB with full stdlib. Automatically stripped.

---

## Development

```bash
cargo build --release                                    # Build everything
cargo build --release -p perry-runtime -p perry-stdlib   # Rebuild runtime (after changes)
cargo test --workspace --exclude perry-ui-ios            # Run tests
cargo run --release -- compile file.ts -o out && ./out   # Compile and run
cargo run --release -- compile file.ts --print-hir       # Debug HIR
```

### Adding a new feature

1. **HIR** — add node type to `crates/perry-hir/src/ir.rs`
2. **Lowering** — handle AST→HIR in `crates/perry-hir/src/lower.rs`
3. **Codegen** — generate LLVM IR in `crates/perry-codegen/src/codegen.rs`
4. **Runtime** — add runtime functions in `crates/perry-runtime/` if needed
5. **Test** — add `test-files/test_feature.ts`

---

## Releasing Perry

Release cadence: patch releases (`0.5.118 → 0.5.119`) ship weekly-ish behind the
macOS CI gate. **Major releases** — any bump of the major or minor number
(e.g. `0.5.x → 0.6.0`, and the upcoming `1.0.0`) — **must be verified on every
supported platform** before the tag is pushed. Patch releases only require the
default CI gate.

### 1. Pre-release checklist (every release)

Run on macOS (the canonical dev host):

```bash
# Full rebuild — runtime/stdlib/UI libs must match the compiler version.
cargo build --release

# Core gates.
cargo test --workspace --exclude perry-ui-ios --exclude perry-ui-tvos \
  --exclude perry-ui-watchos --exclude perry-ui-gtk4 \
  --exclude perry-ui-android --exclude perry-ui-windows
./run_parity_tests.sh                       # Perry vs node stdout parity
./scripts/run_doc_tests.sh                  # Compile + run every docs/examples/*.ts
```

Then bump and tag:

```bash
# Edit Cargo.toml workspace.package.version + CLAUDE.md "Current Version".
# Add a "Recent Changes" entry in CLAUDE.md.
git commit -am "release: v0.x.y"
git tag v0.x.y && git push --tags
```

The `release-packages.yml` workflow fires on the pushed tag and builds the
cross-platform matrix (see §3).

### 2. Major-release verification (all platforms)

Before tagging a major/minor bump, these must all pass:

| Platform | What to run | Runs in CI? |
|---|---|---|
| **macOS** (arm64 + x86_64) | `cargo test` + `run_parity_tests.sh` + `scripts/run_doc_tests.sh` | Yes, `test.yml` (arm64 only) |
| **Linux glibc** (x86_64 + aarch64) | Same, under `xvfb-run -a` for UI; `apt install libgtk-4-dev libadwaita-1-dev xvfb` first | Partial — release build only |
| **Linux musl** (x86_64 + aarch64) | Release build via `release-packages.yml`; spot-check a compiled `hello.ts` runs on Alpine | Build only |
| **Windows** (x86_64 MSVC) | `scripts/run_doc_tests.ps1`; smoke-test `perry compile hello.ts -o hello.exe && .\hello.exe` | Build only |
| **iOS Simulator** | `perry compile --target ios-simulator examples/widget_demo.ts && xcrun simctl install booted out.app` | No (Xcode required) |
| **visionOS Simulator** | `perry compile --target visionos-simulator ...`, launch in Apple Vision Pro Simulator | No (Xcode required) |
| **tvOS Simulator** | `perry compile --target tvos-simulator ...`, launch in Simulator | No (Xcode required) |
| **watchOS Simulator** | `perry compile --target watchos-simulator ...` — requires `rustup toolchain install nightly` + `cargo +nightly -Zbuild-std` | No (Xcode + nightly required) |
| **Android** | `perry compile --target android examples/widget_demo.ts`; install APK on emulator | No (NDK required) |
| **Web / WASM** | `perry compile --target web examples/wasm_ui_demo.ts`, open `out.html` in a browser | No |
| **Home-screen widgets** | `perry compile --target widgetkit ... && perry publish ios` | No |

For v1.0, expect to spend half a day spinning through the four OS VMs locally.
Linux + Windows doc-tests are automated in `test.yml`; the mobile/watch/web
lanes remain manual pending tier-2 simulator orchestration.

### 2a. Simulator-run recipe (iOS / tvOS)

`perry-ui-ios` and `perry-ui-tvos` honor `PERRY_UI_TEST_MODE=1` — when set,
the app renders one frame, optionally writes a screenshot to
`$PERRY_UI_SCREENSHOT_PATH`, and exits cleanly. Combine with
`xcrun simctl` to verify a doc-example runs without a human:

```bash
# Compile for the simulator
perry compile --target ios-simulator docs/examples/ui/counter.ts -o counter.app

# Boot a device (one-time; reuse the UDID across runs)
xcrun simctl boot "iPhone 15"
open -a Simulator

# Install + launch with test mode
xcrun simctl install booted counter.app
PERRY_UI_TEST_MODE=1 \
  PERRY_UI_TEST_EXIT_AFTER_MS=500 \
  PERRY_UI_SCREENSHOT_PATH="$PWD/counter-ios.png" \
  xcrun simctl launch --console booted com.example.counter

# App exits 0 after rendering; screenshot lands at counter-ios.png
```

Same recipe works for `tvos-simulator` + `"Apple TV"` device. On watchOS the
Rust Tier-3 toolchain requires `+nightly -Zbuild-std` — see the
`watchos-simulator` row in the matrix above.

### 3. What CI does on the tag

The `Release Packages` workflow (`.github/workflows/release-packages.yml`)
triggers on a published GitHub Release or manual `workflow_dispatch`. Matrix
runners build:

- `macos-14` / `macos-15` — arm64 + x86_64 Darwin binaries
- `ubuntu-22.04` / `ubuntu-24.04-arm` — glibc x86_64 + aarch64
- `ubuntu-22.04` / `ubuntu-24.04-arm` — musl x86_64 + aarch64
- `windows-latest` — x86_64 MSVC

Artifacts are published to:

1. **npm** (`@perryts/perry` + seven per-platform optional-deps) — via OIDC
   Trusted Publisher
2. **Homebrew** — formula auto-update
3. **APT** (Debian/Ubuntu) — GPG-signed repository
4. **winget** — manifest auto-update
5. **hub.perryts.com** — worker notification so cloud build workers refresh

A tag push with a failing platform build aborts the publish step for that
platform only; fix-forward with a new patch tag (e.g. `v0.6.1`) rather than
amending the existing one.

### 4. Release gates (what blocks a release)

- Parity tests must clear the threshold in `test-parity/threshold.json`
- `cargo test --workspace` (macOS excluded list as above) must be green
- `compile-smoke` must compile every file under `test-files/`
- `doc-tests` must compile + run every example under `docs/examples/`
- Benchmark regressions in `benchmark.yml` hard-fail on release tags (warn only
  on main-branch pushes)

### 5. If a release goes wrong

- **Wrong artifact published**: tag a new patch release with the fix; npm
  rejects re-publishes of the same version anyway.
- **Broken binary on one platform**: the `release-packages.yml` matrix is not
  `fail-fast: true`, so other platforms still publish. Ship a follow-up patch
  for the broken one.
- **CI hook failed after tag**: run `workflow_dispatch` with
  `publish_npm: true` to retry the npm step.

---

## License

MIT
