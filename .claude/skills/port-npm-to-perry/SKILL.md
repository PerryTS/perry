---
name: port-npm-to-perry
description: Port an npm package to run under Perry — audit it for TypeScript-subset gaps, add it to perry.compilePackages, and patch whatever breaks so the package compiles natively. EXPERIMENTAL — feedback welcome at github.com/PerryTS/perry/issues/115.
argument-hint: [package name or path, e.g. "@noble/curves" or "./node_modules/some-pkg"]
allowed-tools: Bash, Read, Edit, Write, Grep, Glob
---

# Port an npm package to Perry

> **Status: experimental.** This skill is a first pass at systematizing what Perry contributors have been doing ad-hoc. Results will vary by package. If it produces bad patches or misses known gaps, please comment on [issue #115](https://github.com/PerryTS/perry/issues/115) — we want to iterate on this.

Your job is to make an npm package compile natively under Perry. Perry compiles a **subset** of TypeScript to native code via LLVM, so not everything runs. The mechanical side — "pull the package into the compile" — is handled by `perry.compilePackages` in `package.json`. The hard part is identifying + patching the constructs Perry doesn't support.

## Inputs

- `$ARGUMENTS`: the package to port. Either a package specifier (`@scope/name`, `name`) or a path to a package directory.

## Steps

### 1. Locate the package

If `$ARGUMENTS` is a specifier, resolve it to `node_modules/<name>` in the current project (or ask the user to run `npm install <name>` first if it's not present). If it's a path, use it directly.

Read `package.json` at the package root. Note:
- `main` / `module` / `exports` — the entry points
- `dependencies` — transitive deps that will also need to port
- `gypfile: true` or a `binding.gyp` file — **stop**: this package has a native C/C++ addon and cannot be compiled natively. Report this to the user and suggest an alternative package (Perry is AOT-only and has no runtime JS/V8 fallback).
- `prebuilds/` directory or `.node` files anywhere — same as above, native addon.

### 2. Scan for known gaps

Perry's TypeScript subset is documented at `docs/src/language/limitations.md`. Run these searches against the package's source:

```bash
# Dynamic code
grep -rn "\beval(" <pkg>/
grep -rn "new Function(" <pkg>/
grep -rn "require(" <pkg>/ | grep -v "^.*://"   # dynamic require
grep -rn "await import(" <pkg>/

# Unsupported primitives
grep -rn "\bSymbol(" <pkg>/
grep -rn "new Proxy(" <pkg>/
grep -rn "new WeakMap(" <pkg>/
grep -rn "new WeakRef(" <pkg>/

# Reflection / metadata
grep -rn "Reflect\." <pkg>/

# Decorators
grep -rn "^@[A-Z]" <pkg>/ --include="*.ts"

# Prototype manipulation
grep -rn "\.prototype\." <pkg>/ | grep -v "Object.prototype"
grep -rn "setPrototypeOf" <pkg>/

# Regex lookbehind (Rust regex crate doesn't support)
grep -rn "(?<=\|(?<!" <pkg>/

# Computed property keys
grep -rn "^\s*\[.*\]:" <pkg>/ --include="*.ts"
```

Make a punch list. For each hit, decide: can it be patched, or does it rule the package out?

### 3. Triage

- **Rules the package out**: native addons, heavy `eval`/`Function`, packages whose core API depends on `Proxy` (e.g., ORMs, validation DSLs).
- **Patchable**: decorators (often removable with a light rewrite), occasional `Symbol` uses (swap for unique string sentinels), lookbehind regex (rewrite as a two-pass match), simple computed keys (hoist to explicit assignment), `Object.setPrototypeOf` in isomorphic fallback paths (often dead code for native targets).
- **Defer to `jsEval` fallback** if patching is unreasonable but only a small surface is affected.

Report the triage to the user before patching anything substantial, so they can decide whether to keep going.

### 4. Add to `perry.compilePackages`

Edit the project's `package.json` to include the package (and any transitive pure-TS/JS deps that also need porting):

```json
{
  "perry": {
    "compilePackages": ["@noble/curves", "@noble/hashes"]
  }
}
```

### 5. Apply patches

Edit the package's files in `node_modules/<name>/` to remove/rewrite the unsupported constructs. Keep the patches minimal and localized — the goal is a clean `perry compile`, not a refactor.

**Record each patch in a new file `perry-patches/<package>.md`** at the project root (create the directory if it doesn't exist) so the user can reapply them after `npm install`. Format: one H2 per patched file, show the before/after. This is the one piece of maintenance overhead Perry adds — call it out explicitly to the user.

### 6. Verify

Run a compile against a small file that imports the package:

```bash
perry compile <test.ts> -o /tmp/port-test && /tmp/port-test
```

If it fails, read the error. Compile-time errors will cite a file:line in the package — jump to it and patch. Runtime errors (e.g., `undefined function`) usually mean a stdlib API is missing; check `docs/src/stdlib/` to see if Perry has the API under a different name.

### 7. Report

Tell the user:
- Which patches were applied (summary — the full record is in `perry-patches/`).
- Any gaps that couldn't be patched and are now `jsEval`-fallbacks (if any).
- Whether transitive deps also needed porting, and which ones.
- A warning if the package is going to need re-patching after every `npm install` — unless/until `compilePackages` gains a patch-file convention natively.

## Important

- **Don't patch blindly.** A grep hit isn't always real — `Symbol` inside a string, `eval` in a comment, `Proxy` as an identifier name, etc. Read the actual usage.
- **Don't commit patches to the user's repo without asking.** Show the diff, let them decide.
- **Respect the experimental label.** If something doesn't work, say so — don't paper over it. Report back so issue #115 gets real feedback.

## Reference

- TS-subset gaps — `docs/src/language/limitations.md`
- Porting guide (human-readable, same content) — `docs/src/packages/porting.md`
- `compilePackages` reference — `docs/src/getting-started/project-config.md`
- JavaScript runtime fallback — `docs/src/stdlib/overview.md` (§ JavaScript Runtime Fallback)
