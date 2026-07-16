# `node:module` granular parity

This lane treats Node.js **26.5.0** as the oracle. Each TypeScript entry point
prints one deterministic contract; controlled CommonJS, ESM, JSON, package, and
source-map fixtures live below this directory and never depend on a registry,
the user home, or global `node_modules`.

## Inventory

The expansion adds **41** entry points, taking the runner-visible lane from **28
to 69**. The 69 entries are grouped by directory as follows:

| Area         | Added | Total | Contracts                                                     |
| ------------ | ----: | ----: | ------------------------------------------------------------- |
| `commonjs`   |     3 |     5 | constructor/prototype shape, wrapper, resolver helpers        |
| `exports`    |     5 |     5 | public names, descriptors, constants, default/static identity |
| `helpers`    |     3 |     6 | compile cache, source-map support, TypeScript stripping       |
| `imports`    |     5 |     6 | CJS namespace identity, JSON, conditional exports             |
| `loader`     |     1 |    19 | hook chaining plus existing registration/dynamic-import cases |
| `methods`    |     4 |     6 | function shapes, builtins, package JSON, builtin ESM sync     |
| `require`    |    13 |    15 | overloads, resolution, cache, cycles, metadata, packages      |
| `source-map` |     7 |     7 | payloads, getters, mappings, origins, validation              |

`loader/fixtures/**/*.ts` contains ten pre-existing executable fixture modules.
The generic runner intentionally counts every `.ts` file recursively, so they
remain represented in the total and baseline rather than being silently
subtracted in this document.

## Primary evidence

The audit is pinned to primary sources, not to latest-branch behavior:

[`EVIDENCE.md`](./EVIDENCE.md) records the per-entry source basis,
Perry/Deno/Bun classification, reproduction commands, and hashes from repeated
runs.

- Node 26.5.0 (`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`):
  [`lib/module.js`](https://github.com/nodejs/node/blob/v26.5.0/lib/module.js),
  [`lib/internal/modules`](https://github.com/nodejs/node/tree/v26.5.0/lib/internal/modules),
  and the upstream
  [`test-module-*` / `test-source-map-*` tests](https://github.com/nodejs/node/tree/v26.5.0/test/parallel).
- Deno 2.9.2 (`356c132ed60e679b34535a287a493193aa8bb6a4`):
  [`ext/node/polyfills/01_require.js`](https://github.com/denoland/deno/blob/v2.9.2/ext/node/polyfills/01_require.js),
  [`ext/node/ops/module.rs`](https://github.com/denoland/deno/blob/v2.9.2/ext/node/ops/module.rs),
  and its pinned
  [`node/module` specifications](https://github.com/denoland/deno/tree/v2.9.2/tests/specs/node).
- Bun 1.2.18 (`0d4089ea7c48d339e87cc48f1871aeee745d8112`):
  [`NodeModuleModule`](https://github.com/oven-sh/bun/tree/bun-v1.2.18/src/bun.js/modules)
  and the pinned
  [`node:module` tests](https://github.com/oven-sh/bun/tree/bun-v1.2.18/test/js/node/module).

Repeated execution of all 69 entries under Node 26.5.0 produced byte-identical
stdout. The focused Perry classification is **29 pass / 69 total**: 37 stable
output differences and three stable compile failures
(`imports/package-conditions.ts`, `require/error-fields.ts`, and
`require/package-boundaries.ts`), with no runtime timeout, compile timeout, or
signal bucket. The floor is measured from those results; failures are retained
to expose claimed-surface gaps rather than weakening the Node assertions.

For the 41 additions, Deno exactly matches 17, completes with an intentional or
implementation-specific output difference on 18, and exits non-zero on 6. Bun
exactly matches 9, completes with a differing result on 18, and exits non-zero
on 14. Node remains authoritative where builtin inventories, namespace
properties, source-map APIs, compile-cache helpers, or error details diverge.

## Isolation rules

- Paths and file URLs printed by fixtures are reduced to semantic suffixes or
  `<cwd>` markers.
- Cache mutations are deleted before exit. Builtin mutations are restored in a
  `finally` block and followed by `syncBuiltinESMExports()`.
- Loader hooks run in their own process and are deregistered.
- Compile-cache probes use a uniquely-created OS temporary directory and remove
  it recursively in `finally`.
- Source-map tests use only controlled inline payloads and normalize resolved
  source URLs. No raw stack or warning text participates in stdout.
- Package fixtures are local, minimal, and cover `main`, `exports`, `imports`,
  condition selection, and hidden-subpath errors without registry access.

## Deliberate stopping boundary

The deterministic public and legacy-public surface claimed by this lane is
covered through exports/descriptors, `Module`, `createRequire`, CommonJS cache
and graph semantics, CJS/ESM/JSON interop, package boundaries, source maps,
loader registration, compile-cache state, TypeScript stripping, and builtin ESM
synchronization. Node 26.5.0 on this build exposes `stripTypeScriptTypes()` but
rejects `mode: "transform"`; the lane records that boundary instead of inventing
transform output from a different build.

Custom loader source generation, permission/policy state, workers, symlink and
realpath matrices, native addons, SEA/snapshots, coverage/inspector coupling,
watch mode, huge graphs, races, resource exhaustion, and platform-specific
global search paths remain separate lanes. Runtime-generated modules and eval
are not used as proxies for AOT module support. These exclusions either require
process flags or host state, duplicate another granular suite, are inherently
platform-sensitive, or cannot distinguish a module parity gap from an AOT
dynamic-code limitation.
