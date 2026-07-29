# Architecture

This is a brief overview for contributors. The rules for creating, retaining,
or removing workspace crates live in the [crate policy](crate-policy.md).

## Compilation Pipeline

```
TypeScript (.ts)
    ↓ Parse (SWC)
    ↓ AST
    ↓ Lower (perry-hir)
    ↓ HIR (High-level IR)
    ↓ Transform (inline, closure conversion, async lowering)
    ↓ Codegen (LLVM)
    ↓ Object file (.o)
    ↓ Link (system cc)
    ↓
Native Executable
```

## Crate Map

| Crate | Purpose |
|-------|---------|
| `perry` | CLI driver, command parsing, compilation orchestration |
| `perry-parser` | SWC wrapper for TypeScript parsing |
| `perry-hir` | HIR types and data structures, plus AST→HIR lowering |
| `perry-transform` | IR passes: function inlining, closure conversion, async lowering |
| `perry-codegen` | LLVM-based native code generation |
| `perry-codegen-wasm` | WebAssembly code generation for `--target web` / `--target wasm` (HIR → WASM bytecode + JS bridge) |
| `perry-codegen-js` | Legacy JavaScript code generator (still present for the JS minifier; the JS-emit `--target web` path was consolidated into `perry-codegen-wasm`) |
| `perry-codegen-swiftui` | SwiftUI code generation for WidgetKit extensions |
| `perry-runtime` | Runtime library: NaN-boxed values, GC, arena allocator, objects, arrays, strings |
| `perry-ffi` | Stable interface used by native binding crates |
| `perry-stdlib` | Runtime-coupled Node.js and Perry standard-library implementations |
| `perry-ext-*` | Independently linked native bindings selected per program |
| `perry-ui` / `perry-ui-model` | Shared UI interface and public model metadata |
| `perry-ui-macos` | macOS UI (AppKit) |
| `perry-ui-ios` | iOS UI (UIKit) |

## Key Concepts

### NaN-Boxing

All JavaScript values are represented as 64-bit NaN-boxed values. The upper 16 bits encode the type tag:

| Tag | Type |
|-----|------|
| `0x7FFF` | String (lower 48 bits = pointer) |
| `0x7FFD` | Pointer/Object (lower 48 bits = pointer) |
| `0x7FFE` | Int32 (lower 32 bits = integer) |
| `0x7FFA` | BigInt (lower 48 bits = pointer) |
| Special constants | undefined, null, true, false |
| Any other | Float64 (the full 64 bits) |

### Garbage Collection

Generational mark-sweep GC, per-thread arena split into nursery + old-gen. Roots come from a precise shadow stack (emitted by codegen), a conservative native-stack scan, and 9 registered runtime scanners. Two-bit aging tenures objects after surviving 2 minor cycles; a write barrier maintains a remembered set for old → young pointers.

See [Internals → Memory Model](../internals/memory-model.md) for the full picture (NaN-boxing, heap layout, root discovery, generational behaviour, env-var escape hatches).

### Handle-Based UI

UI widgets are represented as small integer handles NaN-boxed with `POINTER_TAG`. Each handle maps to a native platform widget (NSButton, UILabel, GtkButton, etc.). Two dispatch tables route method calls and property accesses to the correct FFI function.

## Source Code Organization

The codegen crate is organized into focused modules:

```
perry-codegen/src/
  codegen.rs       # Main entry, module compilation
  types.rs         # Type definitions, context structs
  util.rs          # Helper functions
  stubs.rs         # Stub generation for unresolved deps
  runtime_decls.rs # Runtime function declarations
  classes.rs       # Class compilation
  functions.rs     # Function compilation
  closures.rs      # Closure compilation
  module_init.rs   # Module initialization
  stmt.rs          # Statement compilation
  expr.rs          # Expression compilation
```

The HIR lowering was split into 8 modules:

```
perry-hir/src/
  lower.rs           # Main lowering entry
  analysis.rs        # Code analysis passes
  enums.rs           # Enum lowering
  jsx.rs             # JSX lowering
  lower_types.rs     # Type lowering
  lower_patterns.rs  # Pattern lowering
  destructuring.rs   # Destructuring lowering
  lower_decl.rs      # Declaration lowering
```

## Next Steps

- [Building from Source](building.md)
- See `CLAUDE.md` in the repository root for detailed implementation notes
