**`require('<builtin>')` linked runtime-only, so stdlib-backed builtins returned `undefined` (#8547) — re-landed with the test-isolation bug that defeated the first attempt.**

```
import * as http from 'node:http';   →  Linking (with stdlib)...   typeof createServer() === "object"
const http = require('http');        →  Linking (runtime-only)...  typeof createServer() === undefined
```

`ctx.native_module_imports` drives `needs_stdlib` and therefore the link mode, but it was populated only by the ESM import walk. A CommonJS `require("http")` never landed in it, so the link came out runtime-only, `perry-stdlib`'s `common/dispatch/init.rs` never ran, `JS_NATIVE_HTTP_DISPATCH` stayed null, and the `("http", "createServer")` arm returned `undefined`. Nothing about it was http-specific.

The lowered HIR cannot answer this: for `require('http')` the CJS shim emits a *runtime* dispatcher that switches over builtin names as string literals, and that switch contains a case for **every** builtin regardless of use — a static scan would match the table, not the call. The fix therefore reads literal call sites via the extractor that already exists for CJS wrapping (`cjs_wrap::extract_require_specifiers`).

Scope, measured — the change can only add stdlib linking for a program that genuinely references a stdlib-backed builtin:

| program | link mode | binary |
|---|---|---|
| `console.log(...)` only | runtime-only (unchanged) | 7.7 MB |
| `require("path")` (runtime-only module) | runtime-only (unchanged) | 7.8 MB |
| `require("http")` | with stdlib (was runtime-only) | 14 MB |

**Why the first attempt (#8548) was reverted, and what actually changes here.** It broke `issue_5247_property_read_source_location` with `undefined reference to perry_wasm_host_*` at link time. The cause was not the detection — it was a pre-existing test-isolation bug this change happened to expose:

`issue_5234_wasm_esm_import` is the only thing in the workspace that builds `perry-runtime` with `perry-runtime/wasm-host`, and it built into the **shared** `target/debug`. That replaces `libperry_runtime.a` with one that compiles `webassembly.rs` in, whose undefined `perry_wasm_host_*` references only `libperry_wasm_host.a` satisfies — and that archive is put on the link line only for programs that themselves reference `WebAssembly.*`. Any other suite in the same `cargo test --workspace` job that links runtime-only against the poisoned archive fails. Whichever build wrote the archive last decided the outcome, which is why this was latent until #8548 perturbed the ordering.

`issue_5234` now builds into a dedicated `target/perry-wasm-host-test`, mirroring what the compiler's own no-auto path already does for exactly this reason (`build_wasm_host_runtime` in `optimized_libs/no_auto.rs` uses `target/perry-wasm-host-runtime` so "the prebuilt libperry_runtime.a is not clobbered").

Verified together in one `cargo test` invocation, which is the configuration that failed before: `issue_4903_listen_callback_deferred` **0/2 → 2/2**, `issue_5247_property_read_source_location` **3/3**, `issue_5234_wasm_esm_import` **1/1**.

Closes #8547.
