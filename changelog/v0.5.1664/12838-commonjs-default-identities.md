### Fixed

- Keep builtin CommonJS default exports distinct from ESM namespaces, including dgram's default/bare/prefixed require identity and absence of a recursive synthetic `default` property. Reuse the canonical default-export policy for constant `createRequire` bindings, preserving callable `module` and correcting scoped module/path/os/async_hooks identities.
- Retain dgram hooks for default-only and computed-require paths. Named `createRequire` imports conservatively retain UDP support; the cold-size cost is not measured.
- Add node-suite coverage for default-property absence and shared builtin identities/calls. Independent Node 26.5.1 FAST/PREBUILT verification improved dgram from 43/57 to 45/58 PASS, preserved all 43 prior PASS, and observed 174 successful phase exits. The harness still exits 1 below its 80% threshold, with 13 unchanged parity failures. Scoped shared-CJS checks are not full-module certification; process/path-submodule and reflection gaps remain, including newly exposed ESM descriptor configurability.

Refs #9202. No module-completion claim.
