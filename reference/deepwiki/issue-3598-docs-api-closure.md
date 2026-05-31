# DeepWiki Response

Repository: `PerryTS/perry`
Question: For PerryTS/perry, what docs and generated API/type surfaces describe Node compatibility gaps and API module aliases, and what files should be updated when globalThis/Web-compatible Node globals are exposed? Focus on docs/runtime-parity-gaps.md, docs/typescript-parity-gaps.md, docs/api/perry.d.ts, docs/src/api/reference.md, and crates/perry-api-manifest.
Search: https://deepwiki.com/search/for-perrytsperry-what-docs-and_62db0bbf-78ba-4415-a192-49129b29636c

**Question**: For PerryTS/perry, what docs and generated API/type surfaces describe Node compatibility gaps and API module aliases, and what files should be updated when globalThis/Web-compatible Node globals are exposed? Focus on docs/runtime-parity-gaps.md, docs/typescript-parity-gaps.md, docs/api/perry.d.ts, docs/src/api/reference.md, and crates/perry-api-manifest.

The Node compatibility gaps and API module aliases in Perry are primarily described in the auto-generated documentation and type surfaces. The `docs/src/api/reference.md` file, generated from the `perry-api-manifest::API_MANIFEST`, lists all supported APIs and flags stubs [1](#ref-q1-1) . Similarly, the `docs/api/perry.d.ts` file provides TypeScript declarations for these APIs, also generated from the API manifest [2](#ref-q1-2) .

When `globalThis`/Web-compatible Node globals are exposed, the files that should be updated are within the `crates/perry-api-manifest` crate, specifically the `src/entries.rs` file, which defines the `API_MANIFEST` [3](#ref-q1-3) . Changes to this manifest will then be reflected in the generated documentation and type definitions.

## Node Compatibility Gaps

Perry's approach to Node.js compatibility is to implement a practical subset of TypeScript and Node.js APIs natively [4](#ref-q1-4) .

### Documentation of Gaps
The `docs/src/packages/porting.md` document provides guidance on porting npm packages to Perry and highlights common compatibility gaps [5](#ref-q1-5) . This includes limitations such as:
*   Lack of support for lookbehind regex [6](#ref-q1-6) .
*   Limited or no support for `Symbol`, `Proxy`, `Reflect`, `WeakMap`, `WeakRef`, and `FinalizationRegistry` [7](#ref-q1-7) .
*   Unsupported decorators [8](#ref-q1-8) .
*   Only static imports are supported, meaning dynamic `require()` or `await import()` are not [9](#ref-q1-9) .
*   Prototype manipulation is not supported [10](#ref-q1-10) .
*   Computed property keys in object literals are not supported [11](#ref-q1-11) .

The `docs/src/language/limitations.md` file also serves as a canonical reference for Perry's limitations [12](#ref-q1-12) .

### API Module Aliases
Perry handles Node.js submodules, such as `node:timers/promises` or `node:stream/promises`, by providing per-export function singletons and per-submodule namespace stubs [13](#ref-q1-13) . These are defined in `crates/perry-runtime/src/node_submodules/mod.rs` [14](#ref-q1-14) .

## Generated API and Type Surfaces

The `perry-api-manifest` crate is the source of truth for the standard library and native APIs that Perry implements [15](#ref-q1-15) .

### `docs/src/api/reference.md`
This Markdown file is auto-generated from the `perry-api-manifest::API_MANIFEST` [16](#ref-q1-16) . It lists all supported API entries across various modules and indicates which are stubs [17](#ref-q1-17) . The generation process is handled by the `emit_markdown` function in `crates/perry-api-manifest/src/emit.rs` [18](#ref-q1-18) .

### `docs/api/perry.d.ts`
This TypeScript declaration file is also auto-generated from Perry's API manifest [19](#ref-q1-19) . It provides type definitions for the supported APIs, allowing IDEs and TypeScript compilers to resolve `perry/*` imports [20](#ref-q1-20) . The `emit_dts` function in `crates/perry-api-manifest/src/emit.rs` is responsible for rendering this file [2](#ref-q1-2) .

## Files to Update for GlobalThis/Web-Compatible Node Globals

When new `globalThis`/Web-compatible Node globals are exposed, the primary file to update is `crates/perry-api-manifest/src/entries.rs` [3](#ref-q1-3) . This file contains the `API_MANIFEST`, which is the central definition of all supported APIs [3](#ref-q1-3) .

The `crates/perry-api-manifest` crate is consumed by:
*   `perry-hir` during HIR lowering to reject unimplemented APIs [21](#ref-q1-21) .
*   `perry-codegen` to keep its native dispatch table aligned [22](#ref-q1-22) .
*   Perry's documentation and `.d.ts` emit to produce an external view of the supported surface [23](#ref-q1-23) .

After updating `crates/perry-api-manifest/src/entries.rs`, the generated documentation (`docs/src/api/reference.md`) and TypeScript declaration file (`docs/api/perry.d.ts`) must be regenerated. This regeneration is part of the CI process, specifically the `api-docs-drift` job, which runs the `./scripts/regen_api_docs.sh` script [24](#ref-q1-24) . This job ensures that any changes to the API manifest are reflected in the generated artifacts and will fail if they are out of sync [25](#ref-q1-25) .

The `perry types` command also writes a current snapshot of the stdlib types to `.perry/types/stdlib/index.d.ts` for editor squiggles [26](#ref-q1-26) . This command uses the `PERRY_STDLIB_DTS` constant, which is embedded from `docs/api/perry.d.ts` [27](#ref-q1-27) .

## Notes
The prompt specifically asked about `docs/runtime-parity-gaps.md` and `docs/typescript-parity-gaps.md`. These files were not found in the provided codebase context. However, the `docs/src/packages/porting.md` and `docs/src/language/limitations.md` documents serve a similar purpose by detailing known limitations and common gaps in Perry's compatibility with Node.js and general TypeScript features. The `CLAUDE.md` file also mentions "TypeScript Parity Status" and "Known categorical gaps" [28](#ref-q1-28) .

Wiki pages you might want to explore:
- [Language Support & TypeScript Compatibility (PerryTS/perry)](/wiki/PerryTS/perry#1.3)
- [Contributing & Development (PerryTS/perry)](/wiki/PerryTS/perry#13)
- [Internationalization (i18n) (PerryTS/perry)](/wiki/PerryTS/perry#7)

View this search on DeepWiki: https://deepwiki.com/search/for-perrytsperry-what-docs-and_62db0bbf-78ba-4415-a192-49129b29636c

## References

<a id="ref-q1-1"></a>
### [1] `docs/src/api/reference.md:1-4`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/api/reference.md#L1-L4)

```markdown
# Supported API Reference

This page is auto-generated from Perry's compile-time API manifest (`perry-api-manifest::API_MANIFEST`). It is the source of truth for what `perry compile` accepts; references to symbols not listed here produce `R005 UnimplementedApi` (issue #463). Stubs (#464) are flagged ⚠ — they link cleanly but no-op at runtime on the chosen target.
```

<a id="ref-q1-2"></a>
### [2] `crates/perry-api-manifest/src/emit.rs:120-122`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-api-manifest/src/emit.rs#L120-L122)

```rust
/// Render the manifest as a TypeScript declaration file (`.d.ts`).
/// Editors that load this get squiggles on unimplemented references
/// before `perry compile` runs.
```

<a id="ref-q1-3"></a>
### [3] `crates/perry-api-manifest/src/lib.rs:24`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-api-manifest/src/lib.rs#L24)

```rust
pub use entries::{API_MANIFEST, NATIVE_MODULES, NODE_SUBMODULES, RUNTIME_ONLY_MODULES};
```

<a id="ref-q1-4"></a>
### [4] `docs/src/packages/porting.md:5-6`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/packages/porting.md#L5-L6)

```markdown
Perry compiles a practical subset of TypeScript. Most pure TS/JS packages can be pulled into a native compile via `perry.compilePackages`, but some will need small patches to avoid the constructs Perry doesn't support. This page is a field guide for doing that port — by hand, or by driving a coding agent with the prompt template below.
```

<a id="ref-q1-5"></a>
### [5] `docs/src/packages/porting.md:1-5`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/packages/porting.md#L1-L5)

```markdown
# Porting npm Packages

> **Status: experimental.** This guide — and the [`port-npm-to-perry` skill](https://github.com/PerryTS/perry/tree/main/.claude/skills/port-npm-to-perry) that ships alongside it — is a first pass at systematizing what Perry contributors have been doing ad-hoc. Results will vary by package. Feedback at [issue #115](https://github.com/PerryTS/perry/issues/115).

Perry compiles a practical subset of TypeScript. Most pure TS/JS packages can be pulled into a native compile via `perry.compilePackages`, but some will need small patches to avoid the constructs Perry doesn't support. This page is a field guide for doing that port — by hand, or by driving a coding agent with the prompt template below.
```

<a id="ref-q1-6"></a>
### [6] `docs/src/packages/porting.md:73-75`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/packages/porting.md#L73-L75)

```markdown
### Lookbehind regex

Perry uses Rust's `regex` crate, which doesn't support lookbehind (`(?<=…)` / `(?<!…)`).
```

<a id="ref-q1-7"></a>
### [7] `docs/src/packages/porting.md:87-106`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/packages/porting.md#L87-L106)

```markdown

Not supported as a primitive. When a package uses `Symbol` as a sentinel (the common case — e.g., for unique keys in a registry), swap for a string:

```text
// Before
const REGISTRY_KEY = Symbol("registry");

// After
const REGISTRY_KEY = "__pkg_registry__";
```

When `Symbol` is used to implement `Symbol.iterator`/`Symbol.asyncIterator`, check whether the iteration is actually reached in your use case — often the class has a `for`-loop method alongside the iterator and you can ignore the iterator path.

### `Proxy`, `Reflect`

Not supported. These are usually load-bearing for the package's public API, so porting is often not feasible. If the `Proxy` is only in an optional path (e.g., dev-mode warnings), delete that branch.

### `WeakMap` / `WeakRef` / `FinalizationRegistry`

Not implemented. Swap `WeakMap` for a regular `Map` if the GC semantics aren't critical for correctness (most caches can tolerate this — they'll just hold references slightly longer).
```

<a id="ref-q1-8"></a>
### [8] `docs/src/packages/porting.md:108-110`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/packages/porting.md#L108-L110)

```markdown
### Decorators

```text
```

<a id="ref-q1-9"></a>
### [9] `docs/src/packages/porting.md:119-121`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/packages/porting.md#L119-L121)

```markdown
### Dynamic `require()` / `await import(…)`

Perry only supports static imports. If a package branches on `typeof require !== "undefined"` for a Node/browser split, pick the branch that works natively and delete the other.
```

<a id="ref-q1-10"></a>
### [10] `docs/src/packages/porting.md:126-128`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/packages/porting.md#L126-L128)

```markdown
// Not supported
Object.setPrototypeOf(obj, proto);
MyClass.prototype.newMethod = function() {};
```

<a id="ref-q1-11"></a>
### [11] `docs/src/packages/porting.md:135-136`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/packages/porting.md#L135-L136)

```markdown
```text
// Not supported
```

<a id="ref-q1-12"></a>
### [12] `docs/src/packages/porting.md:71`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/packages/porting.md#L71)

```markdown
Perry's [full limitations list](../language/limitations.md) is the canonical reference. In practice, these are the ones you hit when porting:
```

<a id="ref-q1-13"></a>
### [13] `crates/perry-runtime/src/node_submodules/mod.rs:14-17`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/node_submodules/mod.rs#L14-L17)

```rust
//! diagnostic. This module ships per-export function singletons whose `typeof`
//! is `"function"`, plus per-submodule namespace stubs whose properties point
//! at the same singletons.
//!
```

<a id="ref-q1-14"></a>
### [14] `crates/perry-runtime/src/node_submodules/mod.rs:1-10`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/node_submodules/mod.rs#L1-L10)

```rust
//! Issue #841 — wire up named exports + namespace imports for five
//! Node.js submodules that Perry's manifest had registered but whose
//! FFI export tables defaulted to a `TAG_TRUE` sentinel cell:
//!
//!   - `node:timers/promises` (setTimeout / setImmediate / setInterval / scheduler.*)
//!   - `node:readline/promises` (createInterface, Interface, Readline)
//!   - `node:stream/promises` (pipeline, finished)
//!   - `node:stream/consumers` (text, json, buffer, arrayBuffer, bytes, blob)
//!   - `node:sys` (deprecated alias for node:util — re-exports format, inspect, etc.)
//!
```

<a id="ref-q1-15"></a>
### [15] `crates/perry-api-manifest/src/lib.rs:1`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-api-manifest/src/lib.rs#L1)

```rust
//! Source-of-truth manifest of stdlib / native APIs Perry implements.
```

<a id="ref-q1-16"></a>
### [16] `docs/src/api/reference.md:3`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/api/reference.md#L3)

```markdown
This page is auto-generated from Perry's compile-time API manifest (`perry-api-manifest::API_MANIFEST`). It is the source of truth for what `perry compile` accepts; references to symbols not listed here produce `R005 UnimplementedApi` (issue #463). Stubs (#464) are flagged ⚠ — they link cleanly but no-op at runtime on the chosen target.
```

<a id="ref-q1-17"></a>
### [17] `docs/src/api/reference.md:3-4`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/src/api/reference.md#L3-L4)

```markdown
This page is auto-generated from Perry's compile-time API manifest (`perry-api-manifest::API_MANIFEST`). It is the source of truth for what `perry compile` accepts; references to symbols not listed here produce `R005 UnimplementedApi` (issue #463). Stubs (#464) are flagged ⚠ — they link cleanly but no-op at runtime on the chosen target.
```

<a id="ref-q1-18"></a>
### [18] `crates/perry-api-manifest/src/emit.rs:13-16`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-api-manifest/src/emit.rs#L13-L16)

```rust
/// Render the manifest as a single combined Markdown reference page.
/// Compiler version is interpolated into the header so consumers can
/// tell at a glance which Perry release the doc was generated from.
pub fn emit_markdown(_perry_version: &str) -> String {
```

<a id="ref-q1-19"></a>
### [19] `crates/perry-api-manifest/src/emit.rs:137`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-api-manifest/src/emit.rs#L137)

```rust
        "// Auto-generated from Perry's API manifest (#465). Do not edit by hand."
```

<a id="ref-q1-20"></a>
### [20] `crates/perry/src/commands/types.rs:100`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry/src/commands/types.rs#L100)

```rust
            println!("\nDone! IDEs and tsc can now resolve perry/* imports.");
```

<a id="ref-q1-21"></a>
### [21] `crates/perry-api-manifest/src/lib.rs:5-7`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-api-manifest/src/lib.rs#L5-L7)

```rust
//! - **perry-hir** consults [`module_has_symbol`] during HIR lowering to
//!   reject references to unimplemented APIs at compile time (#463).
//! - **perry-codegen** keeps its native dispatch table aligned with this
```

<a id="ref-q1-22"></a>
### [22] `crates/perry-api-manifest/src/lib.rs:8-9`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-api-manifest/src/lib.rs#L8-L9)

```rust
//!   manifest via a CI test (`tests/manifest_consistency.rs`) — the
//!   manifest is the entry list, codegen owns the dispatch metadata.
```

<a id="ref-q1-23"></a>
### [23] `crates/perry-api-manifest/src/lib.rs:10-11`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-api-manifest/src/lib.rs#L10-L11)

```rust
//! - **perry's docs / .d.ts emit** iterates entries to produce an
//!   external view of the supported surface (#465).
```

<a id="ref-q1-24"></a>
### [24] `.github/workflows/test.yml:102-103`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/.github/workflows/test.yml#L102-L103)

```
      - name: Regenerate API docs
        run: ./scripts/regen_api_docs.sh
```

<a id="ref-q1-25"></a>
### [25] `.github/workflows/test.yml:109-111`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/.github/workflows/test.yml#L109-L111)

```
            echo "::error::API docs drift detected. The compile-time manifest in"
            echo "::error::crates/perry-api-manifest/src/entries.rs changed but the"
            echo "::error::generated artifacts under docs/ weren't regenerated."
```

<a id="ref-q1-26"></a>
### [26] `docs/po/ja.po:4463-4465`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/docs/po/ja.po#L4463-L4465)

```
"reference.md). The `perry types` command writes a current snapshot to "
"`.perry/types/stdlib/index.d.ts` for editor squiggles."
msgstr "マニフェストから自動生成: [`docs/src/api/reference.md`](../api/reference.md)。`perry types`コマンドはエディタのスクィグル用に現在のスナップショットを`.perry/types/stdlib/index.d.ts`に書き込みます。"
```

<a id="ref-q1-27"></a>
### [27] `crates/perry/src/commands/types.rs:37`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry/src/commands/types.rs#L37)

```rust
const PERRY_STDLIB_DTS: &str = include_str!("../../../../docs/api/perry.d.ts");
```

<a id="ref-q1-28"></a>
### [28] `CLAUDE.md:14-26`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/CLAUDE.md#L14-L26)

```markdown
## TypeScript Parity Status

Tracked via the gap test suite (`test-files/test_gap_*.ts`, 28 tests). Compared byte-for-byte against `node --experimental-strip-types`. Run via `/tmp/run_gap_tests.sh` after `cargo build --release -p perry-runtime -p perry-stdlib -p perry`.

**Last full sweep:** run `./run_parity_tests.sh` for the current snapshot. The umbrella tracker is #793 (Node.js + TypeScript compatibility roadmap); the previously-cited #447–#452 batch closed on 2026-05-04. Currently-open trackers worth knowing about:

- **Effect framework end-to-end (#321)** — `#684` (Schema.ts ~310th-init `(number).slice` regression) and `#809` (object-literal computed-keys + cross-module spread) are the live HashRing/Schema blockers.
- **Async context** — `#788` (real `AsyncLocalStorage` tracking across `await`/microtasks/timers) and `#789` (real `async_hooks.createHook` lifecycle + asyncId). Today these are name-only stubs.
- **Compile-as-package** — `#348` (ink TUI end-to-end), `#488/#489` (Drizzle + MySQL), `#678` (linker emits native callsites for V8-fallback modules).
- **Test/CI mechanics** — `#794` (per-category parity thresholds), `#796` (gap-suite output truncation + O(n²) `normalize_output`), `#812` (42-module behavioral matrix), `#806/#807/#808` (test harnesses for mixins / async context / ≥300-init scale).
- **Skip-list audit** — `#797` covers `test-parity/known_failures.json` provenance (issue # + date per entry).

**Known categorical gaps**: lookbehind regex (Rust `regex` crate), `console.dir`/`console.group*` formatting, lone surrogate handling (WTF-8).
```
