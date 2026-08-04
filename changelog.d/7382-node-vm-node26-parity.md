### Fixed

- **Completed Node.js 26.5.0 parity for `node:vm`.** Contexts now preserve
  sandbox, lexical, descriptor, strict-write, code-generation, microtask, and
  cross-realm behavior; `Script`, `compileFunction`, cached-data metadata, and
  experimental VM modules now match the Node oracle across the full 64-case
  module suite.
