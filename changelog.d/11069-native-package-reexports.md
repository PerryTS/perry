### Fixed

- Named re-exports from bundled native packages now emit getter-backed values
  instead of unresolved function-wrapper references. This allows packages such
  as `ethers`, whose internal WebSocket shim uses `export { WebSocket } from
  "ws"`, to link successfully.
