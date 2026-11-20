Fixed `axios` (and any `perry.compilePackages` target with a similar shape)
throwing `TypeError: Class extends value is not a constructor` at
module-init time. The blocker was in the `https-proxy-agent` -> `agent-base`
dependency chain: `agent-base`'s TypeScript source merges `namespace
createAgent { export class Agent extends EventEmitter { ... } }` onto a
same-named `function createAgent()` and exports it with `export =` —
Perry's HIR doesn't correctly attach the namespace's exported members to
the same runtime value `export =` ends up exporting, so
`require("agent-base").Agent` read back as `undefined` and the downstream
`class HttpsProxyAgent extends agent_base_1.Agent` threw. Perry's
`compilePackages` module resolution now detects this TS
namespace/function-merge + `export =` shape and falls back to the
package's compiled JS emit instead of its raw `.ts` source — the same file
Node itself runs, since `--experimental-strip-types` can't execute raw
`namespace`/`export =` syntax either. Extends the existing #6586
ESM+CJS-epilogue fallback in `is_hybrid_cjs_emit_input` with a second,
narrowly-scoped trigger; not keyed on the `agent-base` package name.
