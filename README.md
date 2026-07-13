# Perry

**One codebase. Every platform. Native performance.**

[![Join the Perry Discord community](https://img.shields.io/badge/Discord-Join%20the%20community-5865F2?logo=discord&logoColor=white)](https://discord.gg/chEmpGdTtZ)

Perry is a native TypeScript compiler written in Rust. It takes your TypeScript and compiles it straight to native executables — no Node.js, no Electron, no browser engine. Just fast, small binaries that run anywhere.

[![Latest release](https://img.shields.io/github/v/release/PerryTS/perry?display_name=tag)](https://github.com/PerryTS/perry/releases/latest) | [Website](https://perryts.com) | [Documentation](https://perryts.github.io/perry/) | [Showcase](https://perryts.com/showcase)

**Community:** [Join the Perry Discord](https://discord.gg/chEmpGdTtZ)

```bash
perry compile src/main.ts -o myapp
./myapp    # that's it — a standalone native binary
```

Perry uses [SWC](https://swc.rs/) for TypeScript parsing and [LLVM](https://llvm.org/) for native code generation. The output is a single binary with no runtime dependencies.

### Node.js compatibility

Perry targets close behavioral parity with Node.js. Against Node's own test suite
(node v26.3.0, 53 `node:*` modules), Perry's native runtime passes **~97%**
(2792 / 2863 cases), and overall Node/TypeScript compatibility sits around **95%**.
Real implementations — not stubs — cover `fs`, `http`/`https`/`http2`, `net`/`tls`,
`dns`/`dgram`, `crypto`, `stream` (+ `stream/web`), `events`, `child_process`,
`cluster`, `worker_threads`, `zlib`, `process`, `async_hooks` /
`AsyncLocalStorage`, `Atomics` / `SharedArrayBuffer` (cross-thread), the WHATWG
web globals (`fetch`, `URL`, streams, `structuredClone`, WebCrypto, …), and ~50
popular npm packages. The remaining gap is a long tail of edge-case options and a
few categorical items (lookbehind regex, some `console` formatting). See
[`docs/runtime-parity-gaps.md`](docs/runtime-parity-gaps.md).

**Perry also compiles `.js` / `.cjs` / `.mjs` / `.jsx` source directly** (parsed as
JavaScript, lowered through the same native pipeline as TypeScript) — no
TypeScript annotations required. There are no guarantees for every dynamic JS
pattern, but plain JavaScript projects compile and run in most cases.

---

## Built with Perry

People are building real apps with Perry today. Here are some highlights:

| Project | What it is | Platforms |
|---------|-----------|-----------|
| [**Bloom Engine**](https://bloomengine.dev) | Native TypeScript game engine — Metal, DirectX 12, Vulkan, OpenGL. Write games in TS, ship native. | macOS, Windows, Linux, iOS, tvOS, Android |
| [**Bloom Jump**](https://bloomengine.dev/showcase) | Free retro pixel platformer — five hand-crafted levels, 60 FPS, original chiptune soundtrack. Built entirely with Bloom Engine. | iOS, iPadOS, macOS, tvOS, visionOS, watchOS, Android |
| [**Mango**](https://github.com/MangoQuery/app) | Native MongoDB GUI. ~7 MB binary, <100 MB RAM, sub-second cold start. | macOS, Windows, Linux, iOS, Android |
| [**Hone**](https://hone.codes) | AI-powered native code editor with built-in terminal, Git, and LSP. | macOS, Windows, Linux, iOS, Android, Web |
| [**Pry**](https://github.com/PerryTS/pry) | Fast, native JSON viewer with tree navigation and search. | macOS, iOS, Android |
| [**dB Meter**](https://dbmeter.app) | Real-time sound level measurement with 60fps updates and per-device calibration. | iOS, macOS, Android |

### Screenshots

**Mango** — Native MongoDB GUI ([source](https://github.com/MangoQuery/app))

<p align="center">
  <img src="docs/images/showcase/mango-explorer.png" width="400" alt="Mango — database explorer view" />
  <img src="docs/images/showcase/mango-editor.png" width="400" alt="Mango — document editor view" />
</p>

**Hone** — AI-powered native code editor ([hone.codes](https://hone.codes)) &nbsp;·&nbsp; **Bloom Jump** — retro pixel platformer ([showcase](https://bloomengine.dev/showcase))

<p align="center">
  <img src="https://hone.codes/screenshot.png" width="400" alt="Hone — AI code editor built with Perry" />
  <img src="docs/images/showcase/bloom-jump.webp" width="400" alt="Bloom Jump — retro pixel platformer built with Bloom Engine" />
</p>

> Have something you've built with Perry? Open a PR to add it here!

---

## Sponsored by

Perry's development is backed by our sponsors. 🙏

<p align="center">
  <a href="https://www.skelpo.com">
    <img src="docs/images/sponsors/skelpo-logo.svg" width="360" alt="Skelpo — premium sponsor" />
  </a>
</p>
<p align="center">
  <strong>💎 <a href="https://www.skelpo.com">Skelpo</a> — Premium Sponsor</strong>
</p>

> Want to support Perry and see your logo here? Get in touch via [perryts.com](https://perryts.com).

---

## Performance

> **As of v0.5.585, fast-math is opt-in.** Perry's default mode emits no `reassoc + contract` per-instruction FMF flags, so f64 arithmetic is bit-exact with Node. `--fast-math` (CLI), `PERRY_FAST_MATH=1` (env), or `"perry": { "fastMath": true }` in `package.json` re-enables the flags. See [`docs/src/cli/fast-math.md`](docs/src/cli/fast-math.md) for the discussion of when it does and doesn't matter. The numbers below are Perry's default mode unless noted.

> **Historical native-language comparison.** The following polyglot table and commentary were measured on 2026-05-14 at Perry v0.5.908. They are retained for historical context and are not the current Node/Bun baseline. Current runtime-peer evidence appears in the generated section below.

| Benchmark           | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun | What it tests |
|---------------------|------:|------:|------:|------:|------:|------:|------:|------:|---------------|
| fibonacci           |   309 |   316 |   309 |   446 |   401 |   278 |   987 |   518 | Recursive function calls (i64 specialization) |
| loop_data_dependent |   225 |   226 |   129 |   128 |   225 |   226 |   226 |   230 | Multiplicative carry through `sum` (genuinely-non-foldable f64) |
| object_create       |     2 |     0 |     0 |     0 |     0 |     5 |     8 |     6 | Object allocation (1M objects, scalar replacement) |
| nested_loops        |    18 |     8 |     8 |    10 |     8 |    10 |    17 |    20 | Nested array access (cache-bound) |
| array_read          |    11 |     9 |     9 |    10 |     9 |    11 |    14 |    16 | Sequential read (10M elements) |
| array_write         |     3 |     7 |     2 |     9 |     2 |     6 |     9 |     6 | Sequential write (10M elements) |

Default Perry runs in the same neighborhood as Rust default `-O`, C++ `-O3`, and Swift `-O` on every row — competitive on integer recursion (`fibonacci` 309 vs Rust 316 / C++ 309), within a tick of native on object allocation thanks to scalar replacement (`object_create`), within a few ms on cache-bound work (`nested_loops`, `array_read`/`array_write`), and matching the no-contract compiled pack on genuinely-non-foldable f64 (`loop_data_dependent` 225 vs Rust 226 / Bun 230 / Node 226). Apple Clang `-O3` and Go default win the `loop_data_dependent` row at 128-129 by fusing `sum * a + b` into a single `FMADDD` instruction (FMA contraction is `-ffp-contract=fast` — a separate knob `--fast-math` deliberately doesn't toggle). Python column omitted to keep the table readable; full numbers in [`benchmarks/polyglot/RESULTS.md`](benchmarks/polyglot/RESULTS.md).

We deliberately don't lead with the trivially-foldable accumulator microbenchmarks (`loop_overhead` / `math_intensive` / `accumulate`) that Perry posted big numbers on through v0.5.584. Those are flag-aggressiveness probes — they measure whether each compiler applied `reassoc + autovectorize` to a `sum += 1.0`-shaped loop, not how fast the resulting loop computes under load. Perry default sits in the no-flags pack (97 / 51 / 97 ms in this sweep) on all three; `--fast-math` recovers 12 / 14 / 34 ms. C++ `-O3 -ffast-math` matches Perry `--fast-math` to the millisecond on the same kernels — same LLVM pipeline, one flag. The full breakdown is in [`benchmarks/README.md`](benchmarks/README.md#optimization-probes-compiler-flag-aggressiveness-not-runtime-perf) and [`polyglot/RESULTS_OPT.md`](benchmarks/polyglot/RESULTS_OPT.md).

### vs Node.js and Bun

<!-- public-node-bun:start -->
Generated from [`benchmarks/results/public-node-bun-v1.json`](benchmarks/results/public-node-bun-v1.json) at Perry commit `36cf04e0147c`.
Lower wall-clock median is better; every row includes complete raw samples and passed correctness checks.

| Benchmark | Perry | Node.js | Bun | Result | What it tests |
|---|---:|---:|---:|---|---|
| factorial | 1555 ms | 99 ms | 99 ms | loss vs both | Modular accumulation |
| method_calls | 82 ms | 11 ms | 8 ms | loss vs both | Class method dispatch |
| closure | 49 ms | 51 ms | 51 ms | win vs both | Closure creation and invocation |
| binary_trees | 6 ms | 7 ms | 8 ms | win vs both | Tree allocation and traversal |
| string_concat | 3 ms | 5 ms | 1 ms | mixed | String append loop |
| prime_sieve | 254 ms | 7 ms | 5 ms | loss vs both | Sieve of Eratosthenes |
| mandelbrot | 23 ms | 26 ms | 30 ms | win vs both | Complex-number iteration |
| matrix_multiply | 2311 ms | 35 ms | 35 ms | loss vs both | Matrix multiplication |
| json_roundtrip | 425 ms | 408 ms | 239 ms | loss vs both | Parse and stringify ~1 MB JSON |

<!-- public-node-bun:end -->

Perry compiles to native machine code via LLVM — no JIT warmup, no interpreter overhead. Key optimizations that apply in both modes: **scalar replacement** of non-escaping objects (escape analysis eliminates heap allocation entirely — object fields become registers), inline bump allocator for objects that do escape, i32 loop counters for bounded array access, integer-modulo fast path (`fptosi → srem → sitofp` instead of `fmod`), elimination of redundant `js_number_coerce` calls on numeric function returns, and i64 specialization for pure numeric recursive functions.

Run the complete public sweep with pinned Node and Bun: `./benchmarks/run_public_baseline.sh`.

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

### Windows (Scoop)

```powershell
scoop bucket add perry-ts https://github.com/PerryTS/perry
scoop install perry-ts/perry
```

The Scoop manifest declares `main/llvm` as a dependency, so `scoop install` automatically pulls the official MSVC-default LLVM toolchain Perry needs for Windows-native object emission. Verify with `perry doctor` after install.

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
- **Linux:** GCC or Clang (see below for your distro)
- **Windows:** MSVC (Visual Studio Build Tools)

Linux linker by distro:

```bash
# Debian / Ubuntu / Pop!_OS / Mint
sudo apt install build-essential

# Arch / Manjaro / CachyOS / EndeavourOS
sudo pacman -S base-devel gcc

# Fedora / RHEL / CentOS Stream
sudo dnf install gcc gcc-c++ glibc-devel

# openSUSE
sudo zypper install -t pattern devel_basis

# Alpine / musl-based
sudo apk add build-base

# Void Linux
sudo xbps-install -S base-devel
```

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

Ready-to-run demos live in their own repo: **[PerryTS/perry-examples](https://github.com/PerryTS/perry-examples)**.

| Example | Stack | What it demonstrates |
|---------|-------|---------------------|
| **[express-postgres](https://github.com/PerryTS/perry-examples/tree/main/express-postgres)** | Express + PostgreSQL | Multi-file routes, middleware (CORS, Helmet), connection pooling, error handling |
| **[fastify-redis-mysql](https://github.com/PerryTS/perry-examples/tree/main/fastify-redis-mysql)** | Fastify + Redis + MySQL | Rate limiting, caching layer, database queries, dotenv config |
| **[hono-mongodb](https://github.com/PerryTS/perry-examples/tree/main/hono-mongodb)** | Hono + MongoDB | Lightweight HTTP framework with document database |
| **[nestjs-typeorm](https://github.com/PerryTS/perry-examples/tree/main/nestjs-typeorm)** | NestJS + TypeORM | Decorator-based architecture, dependency injection |
| **[nextjs-prisma](https://github.com/PerryTS/perry-examples/tree/main/nextjs-prisma)** | Next.js-style + Prisma | ORM integration, database migrations |
| **[koa-redis](https://github.com/PerryTS/perry-examples/tree/main/koa-redis)** | Koa + Redis | Middleware composition, session storage |
| **[blockchain-demo](https://github.com/PerryTS/perry-examples/tree/main/blockchain-demo)** | Custom | Blockchain implementation in pure TypeScript |

```bash
git clone https://github.com/PerryTS/perry-examples
cd perry-examples/fastify-redis-mysql
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

**11 target outputs from one codebase:**

| Platform | Backend | Target Flag |
|----------|---------|-------------|
| macOS | AppKit (NSView) | *(default on macOS)* |
| iOS / iPadOS | UIKit | `--target ios` / `--target ios-simulator` |
| visionOS | UIKit (2D windows) | `--target visionos` / `--target visionos-simulator` |
| tvOS | UIKit | `--target tvos` / `--target tvos-simulator` |
| watchOS | WatchKit | `--target watchos` / `--target watchos-simulator` |
| Android | Android Views (JNI) | `--target android` |
| Wear OS | Android Views (JNI) | `--target wearos` |
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
perry compile src/main.ts --target wearos -o MyApp       # Wear OS (Android on a watch)

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
| Decorators | ⚠️ Legacy TS decorators + `emitDecoratorMetadata` ([details](docs/src/language/limitations.md#decorators)) |

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
| **Container** | perry/container (OCI container management) |

---

## Container Module

Perry includes a native container management module `perry/container` for creating, running, and managing OCI containers:

```typescript
import { run, list, composeUp } from 'perry/container';

// Run a container
const container = await run({
  image: 'nginx:alpine',
  name: 'my-nginx',
  ports: ['8080:80'],
});

// List containers
const containers = await list();
console.log(containers);

// Multi-container orchestration
const compose = await composeUp({
  services: {
    web: { image: 'nginx:alpine' },
    db: { image: 'postgres:15-alpine' },
  },
});
```

**Platform support:**
- macOS/iOS: Podman (apple/container support coming soon)
- Linux: Podman (native)
- Windows: Podman Desktop (experimental)

See `example-code/container-demo/` for a complete example.

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
- **Generational Mark-Sweep GC** — per-thread nursery + old-gen arenas, precise shadow stack + conservative stack scan, two-bit aging, 8-byte GcHeader per alloc
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
├── benchmarks/                 # Benchmark suite (Perry vs Node.js vs Bun)
├── packages/                   # npm packages (@perryts/threads)
└── test-files/                 # Test suite
```

---

## Runtime Characteristics

- **Garbage Collection** — generational mark-sweep GC, per-thread nursery + old-gen arenas, precise shadow stack + conservative stack scan, two-bit aging tenures objects after 2 minor cycles, 8-byte GcHeader per allocation
- **Single-Threaded by Default** — async I/O on Tokio workers, callbacks on main thread. Use `perry/thread` for explicit multi-threading.
- **No Runtime Type Checking** — types erased at compile time. Use `typeof` and `instanceof` for runtime checks.
- **Small Binaries** — ~330KB hello world, ~48MB with full stdlib. Automatically stripped.

---

## Privacy & Telemetry

Perry ships **two independent opt-in channels** for sending data home —
nothing leaves your machine without an explicit `enabled = true` or `on` in
`~/.perry/config.toml`. Both honour `PERRY_NO_TELEMETRY=1` and `CI=true`.

### 1. Generic usage analytics — `telemetry.enabled`

Counts `perry compile`, `perry init`, `perry publish` invocations on a
background HTTP POST. Sends: command name, platform (`darwin`/`linux`/...),
Perry version, success/error status, and an anonymous client UUID.

### 2. Compatibility reports — `telemetry.compatibility_reports` (#849)

Separate opt-in for "I hit an unsupported TS/Node feature and bailed." Sends
a structured report when the compiler emits one of these diagnostic codes:
`UnsupportedBinaryOp`, `UnsupportedExpression`, `UnsupportedStatement`,
`DynamicPropertyAccess`, `ImplicitCoercion`, `UnresolvedImport`, `NoOpStub`.

Three modes:

- `off` — never send. Sink isn't even installed; zero overhead.
- `ask` (default) — when a qualifying diagnostic fires, prompt once per
  session: `[y] just this once / [a] always / [n] not this time / [N] never`.
- `on` — always send (after dedup + redaction). No prompt.

**What's sent (the entire payload schema):**

```json
{
  "perry_version": "0.5.x",
  "client_id": "uuid",
  "code": "UnsupportedExpression",
  "category": "gap-categorical",
  "stage": "hir-lower",
  "snippet_hash": "sha256:...",
  "snippet_redacted": "let <id1> = await <id2>();",
  "ts_feature": "decorator",
  "node_api": "node:async_hooks.createHook",
  "os": "darwin-arm64",
  "node_target": "20"
}
```

**What's NEVER sent:** raw source, file paths, project names, env vars, your
program's stdout/stderr, dependency tree, or anything tied to identity
beyond the existing anonymous `client_id`. Snippets are redacted before
hashing — string literals → `"<str>"`, numbers → `<num>`, identifiers
(except built-ins like `console`, `Math`, `Promise`) → `<id1>`, `<id2>`,
capped at 200 chars, with a hard reject if any invariant fails.

A local 30-day dedup cache at `~/.perry/.report-cache` prevents resending
the same `snippet_hash` on every reload.

### Inspecting & managing

```bash
perry doctor                          # shows current mode, sent/queued counts
perry doctor --show-pending-reports   # print redacted payloads queued this run
perry doctor --clear-report-cache     # wipe the 30-day dedup cache
```

To opt out at the file level, edit `~/.perry/config.toml`:

```toml
[telemetry]
enabled = false                  # generic analytics off
compatibility_reports = "off"    # #849 compat reports off
```

---

## Development

```bash
cargo build --release                                    # Build everything
cargo build --profile perry-dev -p perry                 # Fast local dev build (#5422)
cargo build --release -p perry-runtime -p perry-stdlib   # Rebuild runtime (after changes)
cargo build --release -p perry-runtime-static -p perry-stdlib-static  # Emit libperry_*.a (#5422)
cargo test --workspace --exclude perry-ui-ios            # Run tests
cargo run --release -- compile file.ts -o out && ./out   # Compile and run
cargo run --release -- compile file.ts --print-hir       # Debug HIR
```

The static archives (`libperry_runtime.a` / `libperry_stdlib.a`) are emitted by
the `perry-runtime-static` / `perry-stdlib-static` wrapper crates, so a plain
`cargo build` no longer produces them as a side effect. A slimmer compiler CLI
is available with `cargo build -p perry --no-default-features --features dev-cli`.
See [docs/src/contributing/building.md](docs/src/contributing/building.md).

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
