# Perry

**Write TypeScript. Ship native. Everywhere.**

Perry compiles the TypeScript you already write into real machine-code executables — for macOS, Windows, Linux, iOS, Android, watchOS, and TV. No Node.js to install. No Electron to bundle. No runtime at all. Just a binary. (The same codebase can also target the web, emitted as JavaScript or WebAssembly.)

[![Latest release](https://img.shields.io/github/v/release/PerryTS/perry?display_name=tag)](https://github.com/PerryTS/perry/releases/latest)
[![Join the Perry Discord community](https://img.shields.io/badge/Discord-Join%20the%20community-5865F2?logo=discord&logoColor=white)](https://discord.gg/chEmpGdTtZ)

[Website](https://perryts.com) · [Documentation](https://perryts.github.io/perry/) · [Showcase](https://perryts.com/showcase) · [Examples](https://github.com/PerryTS/perry-examples)

```bash
perry compile src/main.ts -o myapp
./myapp    # a standalone native binary — ~330 KB for hello world
```

## The language you know. The deployment you wish it had.

Millions of developers write TypeScript every day — but shipping it has always meant shipping a JavaScript engine: a Node install on every server, ~100 MB of embedded runtime per CLI, or a whole browser engine per desktop app. Perry removes the engine. SWC parses your code, LLVM compiles it to machine code, and you get what systems languages get: instant cold starts, tiny self-contained binaries, real threads — without leaving TypeScript.

|  | **Perry** | Node.js | Bun | Electron |
|---|---|---|---|---|
| **What you ship** | One native binary, from ~330 KB | Your code + a Node install | One binary embedding the JS engine | App bundle with a browser engine |
| **Execution** | Ahead-of-time machine code | JIT | JIT | JIT |
| **Cold start** | Instant — no engine to boot, no warmup | Engine boot + warmup | Engine boot + warmup | Browser boot |
| **Native UI (no WebView)** | ✅ AppKit, UIKit, Android Views, Win32, GTK4 | — | — | Chromium |
| **iOS · Android · watchOS · TV** | ✅ from the same codebase | — | — | — |
| **Multicore** | Real OS threads, data-race-safe at compile time | worker_threads | workers | processes |

<sub>Table describes Perry's native targets. `--target web` / `--target wasm` emit JavaScript / WebAssembly that runs in the browser rather than as a native binary.</sub>

## Performance

Ahead-of-time machine code means no engine boot and no JIT warmup — and on measured workloads, real multiples over the JS runtimes. Highlights from our open benchmark harnesses (Apple M1 Max, medians of repeated runs):

| Workload | **Perry** | Node.js | Bun | Rust |
|---|---:|---:|---:|---:|
| Image convolution (4K, 5×5 Gaussian) | **354 ms** | 1,207 ms — *3.4× slower* | 915 ms — *2.6× slower* | 392 ms |
| Fibonacci (recursive calls) | **309 ms** | 987 ms — *3.2× slower* | 518 ms — *1.7× slower* | 316 ms |
| JSON pipeline (100 records) | **39 ms** | 144 ms — *3.7× slower* | 51 ms — *1.3× slower* | 34 ms |
| Object allocation (1M objects) | **2 ms** | 8 ms — *4× slower* | 6 ms — *3× slower* | <1 ms |
| Array write (10M elements) | **3 ms** | 9 ms — *3× slower* | 6 ms — *2× slower* | 7 ms |
| Peak memory (JSON pipeline) | **3.5 MB** | 36 MB — *10× more* | 11 MB — *3× more* | 1.2 MB |

Look at the Rust column again: on convolution, fibonacci, and the array-write loop, TypeScript compiled with Perry **runs even with — or ahead of — Rust**. And Electron isn't in the table because it doesn't compete here: it ships a whole browser engine per app, where a comparable Perry app is a single-digit-MB native binary.

<sub>Sources: convolution & JSON from the [systems-language report](benchmarks/honest_bench/REPORT.md); fibonacci, object allocation & array write from the [polyglot sweep](benchmarks/polyglot/RESULTS.md) (Perry default mode, no fast-math).</sub>

We publish *everything*, including the workloads where V8's JIT still beats us — no cherry-picked table can survive an open harness. Run it yourself: `./benchmarks/run_public_baseline.sh` ([methodology](benchmarks/README.md)).

## Why developers pick Perry

- **⚡ Native speed, zero warmup.** LLVM-optimized machine code with escape analysis, scalar replacement, and a generational GC — see the [numbers above](#performance).
- **📦 Binaries you can actually email.** Hello world is ~330 KB. Perry links only the runtime your program uses. A real MongoDB GUI built with Perry ships as [a ~7 MB app](https://github.com/MangoQuery/app).
- **🔌 Your Node code mostly just works.** ~97% pass rate on Node's own test suite across 53 `node:*` modules — real implementations of `fs`, `http`/`http2`, `net`/`tls`, `crypto`, `stream`, `child_process`, `worker_threads`, `fetch` and the web globals, plus ~50 popular npm packages (Fastify, Express, mysql2, pg, ioredis, ws, bcrypt, jsonwebtoken…). Plain JavaScript compiles too.
- **🖥️ One UI codebase, 11 targets.** A SwiftUI-like API that compiles to *real* platform widgets — macOS, iOS, iPadOS, visionOS, tvOS, watchOS, Android, Wear OS, Windows, Linux, Web/WASM. Even [home-screen widgets](https://perryts.github.io/perry/widgets/overview.html).
- **🧵 All your cores, safely.** `parallelMap`, `parallelFilter`, and `spawn` on real OS threads — the compiler rejects shared mutable state, so data races don't compile.
- **🔋 Batteries included.** Databases, WebSockets, containers, i18n, keychain, notifications, auto-update, a TUI framework — natively implemented and statically linked.

## Get started in 60 seconds

```bash
# Install (macOS · Linux · Windows)
npm install -g @perryts/perry     # or: brew install perryts/perry/perry
                                  # or: winget install PerryTS.Perry

# Create and run a project
perry init my-app && cd my-app
perry run .
```

npm packages and ES modules work as you'd expect:

```typescript
import fastify from 'fastify';

const app = fastify();
app.get('/api/users', async () => [{ id: 1, name: 'Alice' }]);
app.listen({ port: 3000 }, () => console.log('Listening on :3000'));
```

```bash
perry compile src/main.ts -o api && ./api    # one binary — no node_modules on the server
```

Same code, other platforms: `--target ios`, `--target android`, `--target web`… full list in the [platforms guide](https://perryts.github.io/perry/platforms/overview.html). APT, Scoop, install script, and source builds are in the [installation guide](https://perryts.github.io/perry/getting-started/installation.html); run `perry doctor` to verify your setup.

## Built with Perry

Real products, shipping today:

| Project | What it is | Platforms |
|---------|-----------|-----------|
| [**Bloom Engine**](https://bloomengine.dev) | Native TypeScript game engine — Metal, DirectX 12, Vulkan, OpenGL | macOS, Windows, Linux, iOS, tvOS, Android |
| [**Mango**](https://github.com/MangoQuery/app) | Native MongoDB GUI — ~7 MB binary, sub-second cold start | macOS, Windows, Linux, iOS, Android |
| [**Hone**](https://hone.codes) | AI-powered native code editor with terminal, Git, and LSP | macOS, Windows, Linux, iOS, Android, Web |
| [**dB Meter**](https://dbmeter.app) | Real-time sound level measurement at 60 fps | iOS, macOS, Android |

<p align="center">
  <img src="docs/images/showcase/mango-explorer.png" width="400" alt="Mango — native MongoDB GUI built with Perry" />
  <img src="https://hone.codes/screenshot.png" width="400" alt="Hone — AI code editor built with Perry" />
</p>

More in the [showcase](https://perryts.com/showcase) — built something with Perry? Open a PR and add it.

## Documentation

Everything else lives in the [docs](https://perryts.github.io/perry/):

- [Getting started](https://perryts.github.io/perry/getting-started/installation.html) — install, hello world, project config
- [Language support](https://perryts.github.io/perry/language/supported-features.html) — supported TypeScript features and [limitations](https://perryts.github.io/perry/language/limitations.html)
- [Native UI](https://perryts.github.io/perry/ui/overview.html) · [Multi-threading](https://perryts.github.io/perry/threading/overview.html) · [Standard library](https://perryts.github.io/perry/stdlib/overview.html)
- [Platforms](https://perryts.github.io/perry/platforms/overview.html) — per-platform guides from macOS to watchOS to WASM
- [CLI reference](https://perryts.github.io/perry/cli/commands.html) — commands, flags, `perry.toml`, [privacy & telemetry](https://perryts.github.io/perry/cli/telemetry.html)
- [Contributing](https://perryts.github.io/perry/contributing/architecture.html) — architecture, [building from source](https://perryts.github.io/perry/contributing/building.html), and the [release process](https://perryts.github.io/perry/contributing/releasing.html)

## Community

Perry is built in the open — come say hi:

- 💬 **[Join the Discord](https://discord.gg/chEmpGdTtZ)** — get help, share what you're building, and talk directly to the people building Perry
- 🐛 [Issues](https://github.com/PerryTS/perry/issues) — bug reports and feature requests
- 🚀 [Showcase](https://perryts.com/showcase) — apps the community is shipping

## Privacy

Telemetry is **opt-in**: nothing leaves your machine unless you explicitly enable it in `~/.perry/config.toml`, and `PERRY_NO_TELEMETRY=1` (or `CI=true`) always wins. What can be sent is anonymous and redacted — never your source, paths, or project names. Inspect it any time with `perry doctor`, and see exactly what's in the payload in the [privacy & telemetry docs](https://perryts.github.io/perry/cli/telemetry.html).

## Sponsors

Perry's development is backed by our sponsors. 🙏

<p align="center">
  <a href="https://www.skelpo.com">
    <img src="docs/images/sponsors/skelpo-logo.svg" width="360" alt="Skelpo — premium sponsor" />
  </a>
</p>
<p align="center">
  <strong>💎 <a href="https://www.skelpo.com">Skelpo</a> — Premium Sponsor</strong>
</p>

Want to support Perry and see your logo here? Get in touch via [perryts.com](https://perryts.com).

## License

MIT
