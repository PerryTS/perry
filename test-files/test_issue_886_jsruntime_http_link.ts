// Regression for issue #886 — when a program mixes V8-fallback JS
// modules with natively-compiled TS that emits a `perry-ext-*` FFI
// symbol, the link step pre-fix dropped both the auto-optimized
// `libperry_stdlib.a` AND every `libperry_ext_*.a` from the command
// line. The "Linking (with stdlib)..." gate still printed, but the
// linker error was:
//
//     Undefined symbols for architecture arm64:
//       "_js_node_http_create_server", referenced from:
//           _perry_closure_<file>_…__N in <file>.o
//       "_js_node_http_res_end", referenced from:
//           _perry_closure_<file>_…__N in <file>.o
//     ld: symbol(s) not found for architecture arm64
//
// Root cause: `crates/perry/src/commands/compile/link.rs::build_and_run_link`
// has three mutually-exclusive branches inside the `!skip_runtime`
// block — (1) `jsruntime_lib = Some(...)`, (2) `ctx.needs_stdlib`, and
// (3) runtime-only. The well-known-bindings emission lived ONLY in
// branch 2. Programs reaching branch 1 (which fires whenever a JS file
// hits the V8 bundle path — typical for `compilePackages: ["express"]`
// because most of express's tree is .js and falls through to V8)
// silently skipped `well_known_libs`, so `libperry_ext_http.a` was
// built by `auto-optimize` but never passed to clang. The fix mirrors
// the well-known emission into branch 1.
//
// What this test exercises:
//   - Static import of a `.js` fixture → `ctx.needs_js_runtime = true`
//     → branch 1 in `link.rs`.
//   - `import { createServer } from "node:http"` + `createServer(...)`
//     + `res.end(...)` → codegen emits `js_node_http_create_server`
//     + `js_node_http_res_end` FFI calls → `perry-ext-http` joins
//     `well_known_libs`.
//
// Pre-fix: link fails with the two undefined symbols above.
// Post-fix: link succeeds. Runtime semantics of the V8 fallback module
// are not the regression here — we just need the link to complete.
//
// If `perry-ext-http.a` isn't on disk in the build environment, the
// well-known emission falls through to the rebuild path (`auto-optimize:
// built …/libperry_ext_http.a`); either way the staticlib ends up on
// the link line post-fix.

// @ts-ignore — the .js fixture has no ambient types
import { fallback_marker } from "./fixtures/issue_886_pkg/index.js";
import { createServer } from "node:http";

// Construct the server (don't `.listen()` — that would block this
// process). Reaching this line means the link succeeded.
const server = createServer((_req: any, res: any) => {
  res.end("ok");
});

console.log("server created:", typeof server === "object");
// `fallback_marker` is bridged through V8; its runtime value is not
// the contract of this test (see the v0.5.931 follow-up issue if the
// import path ever needs to be a runtime-validating test).
console.log("fallback typeof:", typeof fallback_marker);
