# @perryts/perry

Native TypeScript compiler. Compiles TypeScript source code directly to native executables via LLVM — no VM, no JIT warmup, no Node at runtime.

## Install

```bash
npm install -g @perryts/perry
# or one-shot
npx @perryts/perry compile hello.ts -o hello && ./hello
```

Installing picks the right prebuilt binary for your platform automatically — `@perryts/perry` declares per-platform packages as `optionalDependencies` and npm (≥8.12) selects the matching one based on `os` / `cpu` / `libc`.

## Supported platforms

| Platform | Package |
|---|---|
| macOS arm64 (Apple Silicon) | `@perryts/perry-darwin-arm64` |
| macOS x64 (Intel) | `@perryts/perry-darwin-x64` |
| Linux x64 (glibc) | `@perryts/perry-linux-x64` |
| Linux arm64 (glibc) | `@perryts/perry-linux-arm64` |
| Linux x64 (musl / Alpine) | `@perryts/perry-linux-x64-musl` |
| Linux arm64 (musl / Alpine) | `@perryts/perry-linux-arm64-musl` |
| Windows x64 | `@perryts/perry-win32-x64` |

### Linux: glibc version

The glibc packages are built on Ubuntu 24.04 (glibc 2.39), so they need **glibc ≥ 2.39**. On an older glibc — Ubuntu 22.04 (2.35), Debian 12 (2.36), RHEL 9 / Amazon Linux 2023 (2.34) — the launcher automatically runs the fully-static musl build instead, which has no libc dependency at all and works everywhere ([#6298](https://github.com/PerryTS/perry/issues/6298)). It prints a one-time notice when it does; set `PERRY_NO_FALLBACK_NOTICE=1` to silence it.

npm only installs the musl package when it thinks the machine is musl-based, so on an old-glibc host you have to ask for it once:

```bash
npm install --force @perryts/perry-linux-x64-musl   # or -arm64-musl
```

The launcher tells you this (with the exact command) instead of letting the binary die with `GLIBC_2.39 not found`. The static build is the same compiler; the one thing it cannot do is build `perry/ui` GTK4 desktop apps.

## Host requirements

Perry produces native binaries by linking its runtime and stdlib (shipped as static archives in the platform package) into your code. That link step uses your system C toolchain, so you need:

- **macOS** — Xcode Command Line Tools (`xcode-select --install`)
- **Linux** — `gcc` or `clang` (e.g. `apt install build-essential` on Debian/Ubuntu, `apk add build-base` on Alpine), plus **clang ≥ 15**: codegen emits opaque-pointer LLVM IR, which clang 14 and older reject with `error: expected type`. Ubuntu 22.04 defaults to clang 14 — `apt install clang-15` and `export PERRY_LLVM_CLANG=/usr/bin/clang-15`.
- **Windows** — MSVC / Visual Studio Build Tools with the C++ workload

Node.js 16 or later is required for the wrapper itself.

## Usage

```bash
perry compile file.ts -o out      # compile to native binary
perry --version                   # print version
perry --help                      # full CLI reference
```

## Links

- Repository: https://github.com/PerryTS/perry
- Issues: https://github.com/PerryTS/perry/issues
- Changelog: https://github.com/PerryTS/perry/blob/main/CHANGELOG.md
