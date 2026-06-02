# JS Runtime Opt-In (`perry.allowJsRuntime`)

Perry refuses to link `perry-jsruntime` — the QuickJS-based runtime
that executes interpreted `.js` files from `node_modules/` — unless
the host application has explicitly opted in. This protects Perry's
primary structural advantage over Node: a Perry binary normally
contains *no* JS evaluator at all.

The check fires at compile time. **Zero runtime cost.**

## How a build hits this

The Perry compiler routes any `.js`/`.cjs`/`.mjs` file from
`node_modules/` through `perry-jsruntime`'s QuickJS sandbox instead of
the native LLVM backend. Most npm packages are pure-JS, so transitive
deps can pull the runtime in without the host author noticing — a
silent regression of Perry's main hardening pitch.

When that happens without an opt-in, the build fails with:

```text
Error: build pulled in `perry-jsruntime` (QuickJS-based eval-equivalent
runtime) via the following file(s):
  - /path/to/node_modules/evilpkg/index.js [evilpkg]

`perry-jsruntime` is treated as a privileged dependency on par with
adding a JIT to the binary — it re-introduces arbitrary runtime code
execution and defeats Perry's structural advantage over Node. Refusing
to link by default. (#499)
```

The diagnostic lists every file that triggered the pull-in, capped at
the first eight, with the owning npm package (when the path resolves
through `node_modules/<pkg>/`).

## Opt-in mechanisms

Three equivalent ways, listed in priority order:

### 1. `perry.allowJsRuntime` in `package.json` (persistent)

```json
{
  "perry": {
    "allowJsRuntime": true
  }
}
```

Recommended for production builds where you've reviewed the JS deps
and decided to ship them. The setting lives in source control next
to the dependency list it affects.

### 2. `--enable-js-runtime` CLI flag (per-invocation)

```bash
perry build src/main.ts --enable-js-runtime
```

Treated as an explicit per-build opt-in. Useful for local
development or one-off builds against a host that intentionally
doesn't set `allowJsRuntime: true`.

### 3. `PERRY_ALLOW_JS_RUNTIME=1` env var (CI-friendly)

```bash
PERRY_ALLOW_JS_RUNTIME=1 perry build src/main.ts
```

`=1`/`true` opts in; `=0`/`false` keeps the refusal on even if
`package.json` opted in — useful as a CI gate that fails closed when
someone tries to merge an opt-in by accident.

## Lockdown mode

This refusal will be part of the deny-set for the upcoming
`--lockdown` compile flag (issue
[`#496`](https://github.com/PerryTS/perry/issues/496)). In lockdown
mode, no opt-in is honored — the build always refuses
`perry-jsruntime` linkage.

## See also

- [`#499`](https://github.com/PerryTS/perry/issues/499) — design discussion.
- The wider supply-chain hardening series
  ([`#495`–`#506`](https://github.com/PerryTS/perry/issues?q=is%3Aissue+label%3Aenhancement+security)).
