**`require('http')` linked runtime-only, so every stdlib-backed builtin reached that way returned `undefined` (#8547).**

Same program, two import forms, opposite outcomes:

```
import * as http from 'node:http';   →  Linking (with stdlib)...   typeof createServer() === "object"
const http = require('http');        →  Linking (runtime-only)...  typeof createServer() === undefined
```

**Cause.** `ctx.native_module_imports` — which drives `needs_stdlib` and therefore the link mode — was populated only by the ESM import walk in `collect_modules.rs`. A CommonJS `require("http")` never landed in it, so the link came out runtime-only, `perry-stdlib`'s `common/dispatch/init.rs` never ran, `JS_NATIVE_HTTP_DISPATCH` stayed null, and the `("http", "createServer")` arm in `native_module_dispatch/dispatch_d_i.rs` took its documented null branch and returned `undefined`. Nothing about this was http-specific: any stdlib-backed builtin reached through `require` behaved the same way.

**Why the obvious fix does not work.** Recovering the requirement from the lowered HIR — the way `uses_dgram` does — fails, and it is worth writing down so nobody retries it. For `require('http')` the module name never becomes a `module:` marker; the CJS shim emits a *runtime* dispatcher that switches over builtin names as string literals, and that switch contains a case for **every** builtin regardless of what the program uses. A static scan of the HIR would match the table rather than the call and link the stdlib into every CJS program.

The fix therefore reads the literal call sites, which is exactly what `cjs_wrap::extract_require_specifiers` already extracts for CJS wrapping: any `require("<literal>")` whose specifier satisfies `perry_hir::requires_stdlib` now feeds `needs_stdlib` / `native_module_imports` alongside the import walk.

**Scope, verified by measurement** — the change can only add stdlib linking for a program that genuinely references a stdlib-backed builtin:

| program | link mode | binary |
|---|---|---|
| `console.log(...)` only | runtime-only (unchanged) | 7.7 MB |
| `require("path")` (runtime-only module) | runtime-only (unchanged) | 7.8 MB |
| `require("http")` | with stdlib (was runtime-only) | 14 MB |

Dynamic `require(someVar)` remains statically undetectable and is unchanged; the generated dispatcher can reach any builtin, so whether that case should force stdlib linking is a separate product decision, noted on #8547.

`crates/perry/tests/issue_4903_listen_callback_deferred.rs` goes 0/2 → 2/2, which clears the last sweep-tier `cargo-test` failure on `main`.

Closes #8547.
