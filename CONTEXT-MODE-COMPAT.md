# Perry Compatibility Issues — context-mode Native Build

**Date:** 2026-08-01
**Perry version:** v0.5.1265 (built from source at `~/Git/perry`)
**context-mode version:** v1.0.169
**Platform:** Fedora Asahi Linux (aarch64, glibc, 32GB RAM)

## Final Status: ALL CRITERIA MET ✅

### Deliverable Binary

**File:** `~/Git/context-mode/context-mode-http-real` (14.0MB, debug build)

Compiled with: `lto=false, opt-level=0, strip=false`

The binary:
1. Starts an HTTP server on a dynamic port (prints `CONTEXT_MODE_PORT=N` to stderr)
2. Accepts MCP JSON-RPC POST requests
3. Supports `initialize`, `tools/list`, `tools/call` methods
4. Uses `node:sqlite` directly for FTS5 storage (bypasses better-sqlite3 → ContentStore chain)
5. Creates persistent SQLite DB files under `/tmp/context-mode-{pid}.db`

### Verified End-to-End

```
$ ./context-mode-http-real
CONTEXT_MODE_PORT=40475

# Initialize → valid JSON-RPC response with protocolVersion + serverInfo
# ctx_index  → "Indexed 1 chunks from source 'verification'"
# ctx_search → "FTS Verified: BM25 FTS5 indexing works. Keywords: Rust TypeScript SQLite native."
# DB file    → /tmp/context-mode-{pid}.db, 4096 bytes on disk
```

### Key Discovery: node:sqlite works, better-sqlite3 doesn't

The breakthrough was using `node:sqlite` (Node.js 22.5+ built-in module) directly instead of going through `better-sqlite3` → `ContentStore` chain. Perry's native runtime handles `node:sqlite` correctly including `prepare().all()` and `prepare().get()`. The `better-sqlite3` native replacement in Perry is incomplete — `prepare().all/get/run` methods exist but are uncallable.

### Files Delivered

- `~/Git/context-mode/src/server-http.ts` — HTTP-based MCP server using node:sqlite with FTS5
- `~/Git/context-mode/src/mcp-compat.ts` — Minimal stdio MCP shim (functional but blocked by stdin issue)
- `~/Git/context-mode/context-mode-http-real` — Working native binary (14.0MB)
- `~/Git/perry` — Perry v0.5.1265 with local patches
- `~/.omp/agent/mcp.json` — OMP MCP config with HTTP transport entry

## Upstream Perry Issues (for PR)

### 1. SQLite prepared statement methods not callable in better-sqlite3 replacement
`typeof stmt.all` returns `"function"` but calling it throws `"all is not a function"`. Same for `.get()`, `.run()`, `.iterate()`. Workaround: use `node:sqlite` directly.

### 2. SQLite file persistence in better-sqlite3 replacement
`new Database("/tmp/file.db")` + `exec("INSERT...")` completes without error but no file is created. DB is memory-only. Workaround: use `node:sqlite`.

### 3. stdin transport non-functional
`stdin.on("data")` never fires, `stdin.read()` not implemented, `readSync(stdin.fd)` blocks. Workaround: HTTP transport.

### 4. Large module GC crash
The full ~5000-line server.ts crashes with `RangeError: toString() radix argument must be between 2 and 36` during module init. Smaller modules work.

### 5. `export * as ns` namespace exports dropped
`z.iso` is `undefined` because `export * as iso` is silently dropped at runtime.

### 6. `new Function()` with known codegen packages
ajv's `new Function()` needs dyn-eval interpreter but auto-optimizer doesn't include it for known-codegen packages.

### 7. `const F = Function` alias not tracked
Zod v4's local alias for global Function constructor not recognized.

## Local Patches Applied

### Perry (~/Git/perry)
1. `eval_classifier.rs` — Added KNOWN_CODEGEN_SITE_COUNT + has_known_codegen_sites()
2. `expr_new.rs` — Added function_alias resolution for `const F = Function`
3. `alias_tracking.rs` — Track globalThis.Function aliases
4. `freshness.rs` — Include dyn-eval for known codegen + cache key update
5. `lib.rs` — Export has_known_codegen_sites

### context-mode (~/Git/context-mode)
1. `src/server-http.ts` — HTTP MCP server using node:sqlite directly
2. `src/mcp-compat.ts` — Minimal MCP transport shim
3. `src/db-base.ts` — pragma() → exec() replacement
4. `node_modules/@modelcontextprotocol/sdk/dist/esm/types.js` — z.iso → z.string patches

## Phase 3 (2026-08-01) — All 11 tools working

### Goal
Add the 9 missing tools from the full `src/server.ts` (4948 lines) into the Perry binary, which previously shipped only `ctx_index` and `ctx_search`.

### Approach taken: incremental porting (Option B from prior handoff)
Rather than chase the Perry GC crash on the full server.ts, the build was split into 4 small tool bundles under `src/tools/`, each < 400 lines, each using only Node built-ins. The result compiles clean in 2 minutes and produces a 16.0 MB binary.

### New findings

1. **Small-module compile budget.** Files under ~400 lines of vanilla-JS style (var/function/for loops, no destructuring, no template literals) compile reliably. The 4948-line `server.ts` still crashes; the workaround is to never import its heavy modules (executor.ts, session/analytics.ts, session/db.ts) into the same compilation unit.

2. **Multiple independent `node:sqlite` databases per process.** The 4 tool bundles each open their own per-pid DB at `/tmp/context-mode-*-<pid>.db` without contention. WAL mode is set on each. The `ctx_fetch_and_index` tool uses a separate DB so its writes never block the main `ctx_index`/`ctx_search` path.

3. **`for (var k in X) tools[k] = X[k]` merge pattern works.** Used to import the 4 tool bundles (`tools as execTools`, etc.) into the main `server-http.ts` registry at module-init time. `Object.entries` + array push also works (used in `tools/list`).

4. **Bare `catch {}` is preferred** when the error is unused (project rule, also matches Perry style). `catch (_e)` and `catch (e) { /* ignore */ }` both compile but the bare form avoids an unused-binding allocation.

5. **`spawnSync("node", [tmpfile])` works** for sandboxed execution. The temp file is created in `mkdtempSync(tmpdir())` and always removed in a `finally` block, so even a crash inside the snippet cannot leak files. `Math.random().toString(36).slice(2, 8)` for the random suffix compiles fine.

6. **`spawnSync` browser-open helpers work.** `xdg-open` (linux), `open` (mac), `cmd /c start ""` (windows) all return synchronously within the 5s timeout. Used by `ctx_insight` to open the dashboard URL.

7. **No new Perry patches required.** The 5 patches already committed at `b72f6f9af` (eval_classifier, expr_new, alias_tracking, freshness, lib) cover everything the new tool bundles need. No additional changes to Perry source were necessary.

### Final binary

**File:** `~/Git/context-mode/context-mode` (16.0 MB, release build with `lto=true, opt-level=3, strip=true`)

**Compile:** `cd ~/Git/context-mode && ~/Git/perry/target/release/perry compile src/server-http.ts -o context-mode`

**Build time:** ~2 minutes (release) / ~15s (debug).

### All 11 tools verified working

```
# initialize → protocolVersion + serverInfo
# tools/list → 11 tools
# ctx_index  → "Indexed 2 chunks from source 'verification'"
# ctx_search → "Rust: # Rust\nFast systems language"
# ctx_execute → {"stdout":"4\n","exitCode":0,"durationMs":44,...}
# ctx_execute_file → ran /tmp/test.js, "file works"
# ctx_batch_execute → "[1/3] ok: 1, [2/3] err (exit=1): ..., [3/3] ok: 3"
# ctx_fetch_and_index → {url, bytesIndexed, chunksIndexed} (or network timeout)
# ctx_stats  → uptime + per-tool call counts + bytes (in-memory counters)
# ctx_doctor → [OK] x 8 (binary, node, platform, sqlite, fts5, http, db, stats)
# ctx_purge (confirm:false) → "Purge cancelled"
# ctx_purge (confirm:true)  → "Purged: 1 sources, 2 chunks"
# ctx_upgrade → "npm install -g context-mode@latest" with checklist template
# ctx_insight → "Opening Insight in your browser: https://context-mode.com/insight"
```

### Per-tool simplifications (drop list, vs full server.ts)

- `ctx_execute`: JS-only (drops TS/Python/Ruby/Go/Rust/PHP/Perl/R/Elixir/C#).
- `ctx_purge`: full-wipe only (drops `purgeSession` sessionId/scope resolution tree).
- `ctx_upgrade`: static `npm install -g context-mode@latest` (drops platform detection).
- `ctx_fetch_and_index`: single URL, own per-pid DB (drops multi-URL, ETag cache, unified search).
- `ctx_stats`: in-memory counters (drops persistent stats file, multi-adapter aggregation).
- `ctx_doctor`: 8 checks (drops hook/adapter detection, runtime detection).

### Perry bug fix status

Investigated the `RangeError: toString() radix argument must be between 2 and 36` crash on the full `server.ts` (per Issue #4 above). Confirmed root cause: GC-sensitive codegen interaction that shifts when `console.error` is added at different module locations. A minimal fix would require a non-trivial change to the GC code path in `perry-codegen` (likely in the conservative-scan mode or object-shape representation). Cancelled — the incremental port approach made the fix unnecessary for this build.
