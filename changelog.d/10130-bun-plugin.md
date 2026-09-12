Fix Bun runtime plugin registration during OpenTUI startup (#10100). Imported
`plugin` and `Bun.plugin` run setup synchronously with inert loader hooks, preserve
async setup completion and errors, and expose a no-op `clearAll`. Deferred JSX
imports explain the missing runtime transform in native builds. The API manifest
and generated reference document the limited support; Bun feature detection and
local shadowing keep their existing behavior.
