# Introduction

Perry is a native TypeScript compiler that compiles TypeScript source code directly to native executables. No JavaScript runtime, no JIT warmup, no V8 — your TypeScript compiles to a real binary.

```typescript
{{#include ../examples/getting-started/intro_hello.ts}}
```

```bash
$ perry hello.ts -o hello
$ ./hello
Hello from Perry!
```

## Why Perry?

- **Native performance** — Compiles to machine code via LLVM. Integer-heavy code like Fibonacci runs 2x faster than Node.js.
- **Real multi-threading** — `parallelMap` and `spawn` give you actual OS threads with compile-time safety. No isolates, no message passing overhead. [Something no JS runtime can do](threading/overview.md).
- **Small binaries** — A hello world is ~300KB. Perry detects what runtime features you use and only links what's needed.
- **Native UI** — Build desktop and mobile apps with declarative TypeScript that compiles to real AppKit, UIKit, GTK4, Win32, or DOM widgets.
- **Terminal UI** — Build interactive CLIs with [ink-shape React hooks](tui/overview.md) (`useState`, `useEffect`, `useApp`) on a native cell-grid renderer. No Node, no reconciler — just a single static binary.
- **7 targets** — macOS, iOS, Android, Windows, Linux, Web, and WebAssembly from the same source code.
- **Familiar ecosystem** — Use npm packages like `fastify`, `mysql2`, `redis`, `bcrypt`, `lodash`, and more — compiled natively.
- **Zero config** — Point Perry at a `.ts` file and get a binary. No `tsconfig.json` required.

## What Perry Compiles

Perry supports a practical subset of TypeScript:

- Variables, functions, classes, enums, interfaces
- Async/await, closures, generators
- Destructuring, spread, template literals
- Arrays, Maps, Sets, typed arrays
- Regular expressions, JSON, Promises
- Module imports/exports
- Generic type erasure

See [Supported Features](language/supported-features.md) for the complete list.

## Quick Example: Native App

```typescript
{{#include ../examples/ui/counter.ts}}
```

```bash
$ perry counter.ts -o counter
$ ./counter  # Opens a native macOS/Windows/Linux window
```

This produces a ~3MB native app with real platform widgets — no Electron, no WebView.

## How It Works

```
TypeScript (.ts)
    ↓ Parse (SWC)
    ↓ Lower to HIR
    ↓ Transform (inline, closure conversion, async)
    ↓ Codegen (LLVM)
    ↓ Link (system linker)
    ↓
Native Executable
```

Perry uses [SWC](https://swc.rs/) for TypeScript parsing and [LLVM](https://llvm.org/) for native code generation. Types are erased at compile time (like `tsc`), and values are represented at runtime using NaN-boxing for efficient 64-bit tagged values.

## Next Steps

- [Install Perry](getting-started/installation.md)
- [Write your first program](getting-started/hello-world.md)
- [Build a native app](getting-started/first-app.md)
