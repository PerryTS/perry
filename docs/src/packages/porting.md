# Porting npm Packages

> **Status: experimental.** This guide — and the [`port-npm-to-perry` skill](https://github.com/PerryTS/perry/tree/main/.claude/skills/port-npm-to-perry) that ships alongside it — is a first pass at systematizing what Perry contributors have been doing ad-hoc. Results will vary by package. Feedback at [issue #115](https://github.com/PerryTS/perry/issues/115).

Perry compiles a practical subset of TypeScript. Most pure TS/JS packages can be pulled into a native compile via `perry.compilePackages`, but some will need small patches to avoid the constructs Perry doesn't support. This page is a field guide for doing that port — by hand, or by driving a coding agent with the prompt template below.

## When porting makes sense

| Situation | Try this first |
|-----------|---------------|
| Package uses Node native addons (`.node` files, `binding.gyp`, `prebuilds/`, `node-gyp`) | **Don't put it in `compilePackages`.** Find a pure JS/TS alternative, or replace the native boundary with a Perry native binding (`perry.nativeLibrary`). |
| Package is pure TS/JS with only light use of dynamic features | **Good candidate.** Add to `compilePackages`, patch whatever trips the compiler. |
| Package's core API is built on `Proxy` (ORMs, validation DSLs, reactive stores) | **Probably not portable.** The surface Perry-users touch is the Proxy. |
| Package is pure TS/JS but uses `Symbol`, `WeakMap`, `console.dir`, etc. | **Patchable.** See [Common gaps](#common-gaps) below. |

## Native addon packages

`compilePackages` is only for JavaScript and TypeScript packages. It
does not make Node native addons portable.

Packages that ship `.node` files, `binding.gyp`, `prebuilds/`, or a
`"gypfile"` package marker are compiled against Node's native addon
ABI. The JavaScript entry point is usually just loader glue around a
shared library that expects Node-API/N-API, NAN, V8, libuv, or Node
internals. Perry does not host that ABI as part of `compilePackages`.
Common examples in this category include `node-pty`, `sharp`,
`better-sqlite3`, and `sqlite3`.

For those packages, use one of these paths instead:

- choose a pure JS/TS dependency that Perry can compile;
- keep the feature unsupported for the native Perry build;
- write or install a thin Perry native binding that exposes the public
  package API through `perry.nativeLibrary`.

The last option should replace only the native boundary. It should not
rewrite a whole npm package in Rust unless the package is already just a
small native facade.

### Known-working packages

These work end-to-end via `compilePackages` with no patches required:

- **`hono`** — the full hono `app.fetch` surface including middleware
  (`hono/logger`, `hono/cors`, `hono/jwt`), route groups, JSON responses.
  Enough for testing and edge-runtime deploys (CF Workers / Vercel Edge /
  Lambda / Deno Deploy). See [HTTP & Networking → Hono](../stdlib/http.md#hono).
  Long-lived HTTP server deployment via `@hono/node-server` or hand-rolled
  `node:http` is currently blocked on [#589](https://github.com/PerryTS/perry/issues/589).
  Closed via issues #421 / #486 / #487 / #577.
- **`@bradenmacdonald/s3-lite-client`** — pure-TS AWS S3 / S3-compatible
  storage client (R2, MinIO, B2, Spaces, Supabase, LocalStack). Full SigV4
  signing chain (Put/Get/Head/Delete/List + presigned URLs) verified
  byte-identical to `bun`. See [HTTP & Networking → AWS S3](../stdlib/http.md#aws-s3--s3-compatible-object-storage)
  for the usage pattern. Closed via [#551](https://github.com/PerryTS/perry/issues/551)
  + 15 general-purpose stdlib fixes (Web Crypto, Web Streams subclassing,
  typed-array marshalling, `extends Error`, namespace imports, etc.).

## The workflow

### 1. Add it to `compilePackages`

In your project's `package.json`:

```json
{
  "perry": {
    "compilePackages": ["@noble/curves", "@noble/hashes"]
  }
}
```

This is what tells Perry to pull the package into the native compile instead of routing it through a JavaScript runtime. See [Project Configuration](../getting-started/project-config.md#compilepackages) for the full semantics — including how first-resolved directories get cached so transitive copies dedup.

### 2. Try compiling

```bash
perry compile src/main.ts -o /tmp/port-test && /tmp/port-test
```

Most of the time this is where you find out what's actually broken. Compile-time errors cite a file:line in the package — that's your patch list.

### 3. Patch the gaps

See [Common gaps](#common-gaps) for the typical fixes. Keep patches minimal and localized — the goal is a clean compile, not a refactor.

**Record each patch** in a file at your project root (convention: `perry-patches/<package>.md`) so you can reapply them after `npm install` blows them away. Until `compilePackages` grows a native patch-file convention, this is the one bit of maintenance overhead.

### 4. Re-check after each compile

Iterate: compile, patch the next error, compile again. Don't try to catch everything in a single pass — some errors only surface after earlier ones are fixed.

## Common gaps

Perry's [full limitations list](../language/limitations.md) is the canonical reference. In practice, these are the ones you hit when porting:

### Lookbehind regex

Supported. Perry compiles with Rust's `regex` crate first and transparently
falls back to `fancy-regex` for patterns the fast engine can't express —
lookbehind (`(?<=…)` / `(?<!…)`), lookahead, and backreferences — including
capture-group translation and replacement-string expansion. No rewrite is
needed when porting a package that uses them.

### `Symbol`

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

### Decorators

```text
// Not supported
@Component
class Foo {}

// Remove the decorator and inline the behavior, or use a factory function
const Foo = Component(class Foo {});
```

### Dynamic `require()` / `await import(…)`

Perry only supports static imports. If a package branches on `typeof require !== "undefined"` for a Node/browser split, pick the branch that works natively and delete the other.

### Prototype manipulation

```text
// Not supported
Object.setPrototypeOf(obj, proto);
MyClass.prototype.newMethod = function() {};
```

Usually appears in fallback shims for older runtimes. Often dead code in the Perry path — just delete it.

### Computed property keys in object literals

```text
// Not supported
const obj = { [key]: value };

// Rewrite
const obj: Record<string, V> = {};
obj[key] = value;
```

## Using a coding agent

A general coding agent (Claude Code, Cursor, Codex, Aider) can drive most of this workflow. If you're using a skill-aware agent, invoke the [`port-npm-to-perry` skill](https://github.com/PerryTS/perry/tree/main/.claude/skills/port-npm-to-perry) directly. Otherwise, paste this prompt:

```
I want to port the npm package <NAME> to run under Perry
(https://github.com/PerryTS/perry). Perry compiles a subset of TypeScript
natively; the subset's gaps are documented at
https://github.com/PerryTS/perry/blob/main/docs/src/language/limitations.md.

Please:

1. Read the package at node_modules/<NAME>/. Check package.json for
   native addons (binding.gyp, gypfile, prebuilds/ — stop if present).
2. Scan for unsupported constructs: eval, new Function, dynamic require,
   Symbol, Proxy, WeakMap, WeakRef, Reflect, decorators, computed
   property keys.
3. Report a triage: what rules the package out vs. what's patchable.
4. If patchable: add the package to perry.compilePackages in
   package.json, apply minimal localized patches, and record each
   patch in perry-patches/<NAME>.md.
5. Verify by running `perry compile` against a small file that imports
   the package.

Don't patch blindly — a grep hit inside a string or comment isn't real.
Show me the triage before applying substantial patches.
```

This is intentionally an agent-agnostic prompt — it'll work with any competent coding agent. The skill version bundles the same instructions with richer context and is auto-discovered by Claude Code.

## Giving feedback

This whole workflow is experimental. If a port fails in a way that feels like Perry should handle it — or if the guide misses a common gap — please comment on [issue #115](https://github.com/PerryTS/perry/issues/115) so we can iterate.
