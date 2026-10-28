### Fixed

- `import * as Mod from "./this-module.js"` now reads as the module's own namespace object from that module's own top-level code; the value was `undefined` because the namespace global was emitted only for modules another module namespace-imports, and its populator ran after the body that reads it. The static-import counterpart of #10160 (#13436).
