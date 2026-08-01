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
