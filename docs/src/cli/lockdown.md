# `--lockdown` — Refuse Arbitrary-Code-Execution Surfaces

A single flag that fails the build if any of the standard arbitrary-
code-execution vectors are reachable from the module graph. Most apps
need none of them; lockdown is a one-line opt-in to "this app is
**provably** free of arbitrary-code-execution vectors."

**Zero runtime cost.** The check runs at compile time, after `collect_modules`,
before any codegen work begins.

**Cross-platform.** Runs in the platform-agnostic `compile_command`
driver, so every backend (LLVM / WASM / ArkTS / HarmonyOS / Glance /
SwiftUI / JS) inherits the protection from one choke point.

## What lockdown refuses

| Surface                                  | Detected via                                      |
|------------------------------------------|----------------------------------------------------|
| `perry-jsruntime` (QuickJS) in graph     | `ctx.needs_js_runtime` flipped during collection. |
| `perry.nativeLibrary` archive reference  | `ctx.native_libraries` non-empty after resolution. |
| `child_process.*` call sites             | HIR walker covers every `ChildProcess*` variant + the general-shape `NativeMethodCall { module: "child_process", … }` fallback. |
| Dynamic stdlib dispatch (`fs[runtimeVar]`) | HIR lowering re-arms the `#503` refusal (`error[U006]`). Allowed by default since [#5263](https://github.com/PerryTS/perry/issues/5263); lockdown turns it back on. |

The `child_process`/jsruntime/nativeLibrary checks run together as a
combined post-collect diagnostic; the dynamic-dispatch refusal is enforced
during HIR lowering (it re-arms the always-existing `#503` pass). The failure
lists every offending surface so the reviewer can address it at once.

## Enabling lockdown (priority order)

1. **CLI flag**: `perry compile --lockdown src/main.ts`. Per-build.
2. **Env var**: `PERRY_LOCKDOWN=1`. CI-friendly. `=0` explicitly
   disables.
3. **`package.json`**: persistent.

   ```json
   {
     "perry": {
       "lockdown": true
     }
   }
   ```

Precedence: package.json → env → CLI (last wins, mirrors `--fast-math`).

## Diagnostic example

```text
Error: `--lockdown` refused the build because the following
arbitrary-code-execution surfaces are reachable:
  - perry-jsruntime (QuickJS-based eval-equivalent) is reachable
    from the module graph — see #499 docs for the matching opt-in
    gate
  - `perry.nativeLibrary` archives referenced by: @bloomengine/engine
  - `child_process.*` reached from 2 call site(s):
      - /repo/src/main.ts: child_process.execSync
      - /repo/lib/foo.ts: child_process.spawn
```

The child_process site list is capped at 12 entries; trailing sites
are summarised as `... and N more`.

## Composing with the rest of the security series

Lockdown is the umbrella mode for the wider supply-chain hardening
series ([`#495`–`#506`](https://github.com/PerryTS/perry/issues?q=is%3Aissue+label%3Aenhancement+security)):

- [`#503`](https://github.com/PerryTS/perry/issues/503) /
  [`#5263`](https://github.com/PerryTS/perry/issues/5263) — refuses
  dynamic stdlib dispatch (`obj[runtimeVar]()`). **Allowed by default**
  (dynamic selection over a linked namespace can only reach already-linked
  members); lockdown re-arms the refusal. An explicit
  `perry.allowDynamicStdlibDispatch: false` / `PERRY_ALLOW_DYNAMIC_STDLIB=0`
  re-arms just this check without the rest of lockdown.
- [`#499`](https://github.com/PerryTS/perry/issues/499) — gates
  `perry-jsruntime` behind explicit host opt-in. Lockdown forces the
  gate to its strict default.
- [`#497`](https://github.com/PerryTS/perry/issues/497) — host
  allowlist for `perry.nativeLibrary` / `compilePackages`. Lockdown
  refuses *any* nativeLibrary reference, no allow-list needed.

## See also

- [`#496`](https://github.com/PerryTS/perry/issues/496) — design discussion.
