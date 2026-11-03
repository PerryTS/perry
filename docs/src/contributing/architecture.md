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
    ↓ Program facts and layout planning
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
| `perry-wpfacts` | Value-flow facts and cross-module contracts |
| `perry-abi` | Shared compiler/runtime value and layout contract |
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

The generic JavaScript value ABI is a 64-bit NaN-boxed word. Proven native values and specialized storage can use other representations; boxing is required when crossing a generic value boundary.

| Tag | Type |
|-----|------|
| `0x7FFF` | String (lower 48 bits = pointer) |
| `0x7FFD` | Pointer/Object (lower 48 bits = pointer) |
| `0x7FFE` | Int32 (lower 32 bits = integer) |
| `0x7FFA` | BigInt (lower 48 bits = pointer) |
| `0x7FF9` | Inline short string |
| `0xFFF9` | Native handle |
| Special constants | undefined, null, true, false |
| Any other | Float64 (the full 64 bits) |

### Garbage Collection

The per-thread tracing collector combines copying nursery collection, non-moving fallbacks and full mark-sweep. Native roots use LLVM statepoints; supported fallback targets use shadow frames. Runtime root scanners cover globals and side tables. Conservative stack scanning is diagnostic-only. Adaptive aging, large young storage and whole-block promotion have separate policies.

See [Internals → Memory Model](../internals/memory-model.md) for the full picture (NaN-boxing, heap layout, root discovery, generational behaviour, env-var escape hatches).

### Handle-Based UI

Native UI registry IDs use `HANDLE_TAG` (`0xFFF9`), distinct from managed heap pointers. Each handle maps to a native platform widget (NSButton, UILabel, GtkButton, etc.). Two dispatch tables route method calls and property accesses to the correct FFI function.

## Source Code Organization

Start with `crates/perry-hir/src/lower/` for AST lowering, `crates/perry-codegen/src/codegen/` for module orchestration, and its sibling `expr/`, `stmt/`, `lower_call/` and `native_root_coverage/` trees for emission. [Program facts](../internals/program-facts.md) connect analysis to layout and specialization. Turnloop supplies the event loop; Tokio is not part of the runtime.

## Adding a native UI widget

A new widget touches four places:

1. Runtime: create the widget in `crates/perry-ui-macos/src/widgets/` and call `register_widget(view)`.
2. FFI: export `perry_ui_<widget>_create` from `crates/perry-ui-macos/src/lib.rs`.
3. Codegen: declare the extern and add the `NativeMethodCall` dispatch in `crates/perry-codegen/src/lower_call/native/native_ui_widgets_branch.rs`.
4. HIR: `crates/perry-hir/src/lower/`, only if the widget has instance methods.

## Next Steps

- [Building from Source](building.md)
- See `AGENTS.md` in the repository root for implementation traps
