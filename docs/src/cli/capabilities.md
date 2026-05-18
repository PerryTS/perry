# Per-Package Capabilities (`perry.permissions`)

A compile-time HIR pass walks every imported dependency's source
modules, derives the capability tokens its stdlib call sites would
need, and refuses the build for any call site whose required token
isn't in the dependency's allow-list (or the `*` default). Host code
is always granted `*` unconditionally — gating host code is the
`--lockdown` mode (#496), not per-package policy. (#501)

**Zero runtime cost** — purely a compile-time refusal. **Cross-platform**:
runs in the platform-agnostic `compile_command` driver before any
backend (LLVM / WASM / ArkTS / HarmonyOS / Glance / SwiftUI / JS) is
invoked.

## Why

Most npm packages will never declare their own capabilities. The
prior art around runtime permission prompts (Deno, Bun) ships a
prompt; that doesn't help when an install-time `bun add` lands a
hostile dep that hides its egress until production. `perry.permissions`
moves the gate to compile time and to the host's `package.json`, so
the supply chain is *static* from the consumer's perspective.

## Host config

```json
{
  "perry": {
    "permissions": {
      "lodash": [],
      "axios": ["net:fetch"],
      "@scope/utils": ["crypto"],
      "*": []
    }
  }
}
```

- Keys are exact npm package names (`@scope/pkg` accepted) or the
  universal `"*"` default.
- Values are arrays of capability tokens (see below). Empty array
  means "this dep is only allowed to compute — no I/O".
- Absent map → pass is disabled and existing builds compile
  unchanged. Set any entry to enable.

## Capability tokens (MVP)

| Token | Stdlib surface |
|-------|----------------|
| `fs:read` | `fs.readFile`, `fs.readFileSync`, `fs.stat`, `fs.readdir`, … |
| `fs:write` | `fs.writeFile`, `fs.appendFile`, `fs.mkdir`, `fs.unlink`, `fs.rm`, … |
| `crypto` | `crypto.*`, `crypto.subtle.*` |
| `proc:env` | `process.env.*` reads |
| `proc:argv` | `process.argv` reads |
| `proc:exec` | `child_process.*` |
| `net:fetch` | `fetch`, `Request`, `Response`, `Headers` |
| `net:listen` | `net.createServer`, `http.createServer`, `https.createServer` |
| `net:connect` | `net.connect`, `net.createConnection`, raw socket clients |
| `*` | Grants every token above. Escape hatch — use sparingly. |

## Diagnostic

A failing build prints a combined diagnostic across every refused
call site (capped at the first 12 entries to keep output reasonable):

```text
Error: per-package capability policy refused 3 stdlib call site(s):
  - `axios` net:fetch at node_modules/axios/lib/http.js:42 requires `net:fetch`
  - `axios` fs:read at node_modules/axios/lib/cookies.js:11 requires `fs:read`
  - `mysterydep` proc:exec at node_modules/mysterydep/cli.js:7 requires `proc:exec`

`perry.permissions` provides a static guarantee that each
dependency only reaches the stdlib surfaces you've explicitly
granted it. Refusing the build. (#501)
```

The output names the owning package, the call kind, the source span,
and the missing token — enough to either (a) extend the allow-list,
(b) set `"*": ["<token>"]` for a wider default, or (c) replace the
dep with one that doesn't need the capability.

## Recommended workflow

1. **Start empty.** Set `"permissions": {}` to confirm your build
   is currently passing without the pass active.
2. **Flip the default to deny.** Add `"*": []` and rebuild. The
   diagnostic enumerates every capability your dep tree currently
   reaches.
3. **Grant minimum tokens per dep.** Use the diagnostic to populate
   `permissions` with the smallest token set each package needs.
4. **Lock in CI.** Once the build is green with the explicit
   permissions, leave it that way — new deps that want new tokens
   show up as build failures, surfacing in the PR review.

## Relationship to other security flags

- **`--lockdown` (#496)** — gates host code itself against the
  arbitrary-code-execution surfaces (perry-jsruntime,
  `perry.nativeLibrary` archives, `child_process.*`). Orthogonal:
  `perry.permissions` is per-dep, `--lockdown` is whole-binary.
- **`allowedHosts` (#502)** — narrows `net:fetch` from "any URL" to
  "URLs matching this allow-list." A dep with `net:fetch` permission
  still has to clear the egress allow-list at every call site.
- **`PERRY_SANDBOX_BUILDRS` (#505)** — sandboxes the *build-time*
  `build.rs` scripts. `perry.permissions` controls what the
  *runtime* binary can do.

## See also

- [`#501`](https://github.com/PerryTS/perry/issues/501) — design discussion.
- [`--lockdown`](./lockdown.md)
- [Egress Allowlist (`allowedHosts`)](./allowed-hosts.md)
- [`PERRY_SANDBOX_BUILDRS`](./sandbox-buildrs.md)
