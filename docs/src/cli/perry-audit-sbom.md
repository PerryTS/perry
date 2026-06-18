# Behavioral SBOM (`perry audit --sbom`)

Every Perry compile writes a behavioral SBOM to `audit.json` in the
project's cache dir (default
`<project>/node_modules/.cache/perry/audit.json`) — a per-module manifest of the
stdlib symbols the build actually calls. The manifest is the
foundation for the rest of the supply-chain hardening series and gives
reviewers a way to see exactly what surface a dependency touches
without rebuilding the binary.

**Zero runtime cost.** The walk runs at compile time over the lowered
HIR; the file is written observationally and a missing-directory
error never fails the build.

## What's recorded

For each source module:

- **`source`** — canonical path the module was lowered from.
- **`package`** — owning npm package name when the source lives
  under `node_modules/<pkg>/...` (scope-aware: `@scope/pkg`).
  `null` for host source.
- **`stdlib`** — map of `<namespace>` → sorted unique method names.
  Captures both the general-shape `NativeMethodCall` lowering
  (`mysql2.createConnection`, `child_process.execSync`, …) and the
  dedicated specialized variants Perry uses for hot paths
  (`fs.readFileSync`, `path.join`, `process.env`, `tty.isatty`,
  `url.fileURLToPath`, …).

## Example

A `main.ts` like:

```typescript,no-test
import * as fs from "fs";
import * as path from "path";

const data = fs.readFileSync("/etc/hostname", "utf8");
const p = path.join("/tmp", "x");
console.log(data, p);
```

produces:

```json
{
  "version": 1,
  "modules": [
    {
      "source": "/repo/main.ts",
      "package": null,
      "stdlib": {
        "fs": ["readFileSync"],
        "path": ["join"]
      }
    }
  ]
}
```

The JSON output is byte-deterministic across builds (BTreeMap keys +
sorted method lists), so `perry audit --sbom > before.txt` + a
`package.json` change + a re-build + `perry audit --sbom > after.txt`
+ `diff before.txt after.txt` is a meaningful review tool — any new
capability a dependency reaches surfaces as added lines.

## CLI

`perry audit --sbom [PATH]`

- Reads the manifest from `audit.json` in the resolved cache dir
  (default `<PATH>/node_modules/.cache/perry/audit.json`; honors
  `--cache-dir` / `PERRY_CACHE_DIR` / perry.toml `[perry] cacheDir` /
  package.json `perry.cacheDir`), walking up
  the directory tree if needed (same shape `perry compile` walks up
  to find `package.json`).
- Default `PATH`: current directory.
- In `--format json` mode dumps the raw manifest pretty-printed.
- In text mode groups modules by owning npm package; host source is
  reported under `<host source>`.
- Returns a clear error if the manifest doesn't exist yet — `perry
  compile` or `perry run` writes it on every successful build.

## What's NOT yet recorded

Scope of this first cut (MVP):

- **Literal `fetch` / `http.get` URLs** — covered separately by
  [`#502`](https://github.com/PerryTS/perry/issues/502) which the
  manifest will graft onto under a `literal_hosts` key.
- **Native-library symbol references** (FFI registry) — tracked in
  the perry-codegen FFI registry and will graft onto the manifest
  under a `native_symbols` key.
- **`perry audit --sbom --diff`** — the bytes-deterministic JSON
  shape already enables the diff workflow via plain `diff` /
  `git diff`; a built-in `--diff` is a follow-up that picks a
  baseline (`audit.last.json` in the cache dir) and pretty-prints the
  change set.

The manifest shape is versioned (`version: 1`) so consumers can
detect when new top-level keys land.

## See also

- [`#495`](https://github.com/PerryTS/perry/issues/495) — design discussion.
- The wider supply-chain hardening series
  ([`#495`–`#506`](https://github.com/PerryTS/perry/issues?q=is%3Aissue+label%3Aenhancement+security)).
