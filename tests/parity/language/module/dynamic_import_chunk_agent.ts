// Helper for test_gap_dynamic_import_reexport_chunk.ts — issue #6304.
//
// A PURE RE-EXPORT chunk: the exact two-statement shape esbuild/bun emit for
// a shared chunk under `--splitting` (`import { … } from "./chunk-XXXX.js";
// export { … };`). Nothing is defined here — every exported name is an IMPORT
// binding whose value lives in the shared chunk.
//
// Pre-fix, `flatten_exports` treated `export { run }` as a LOCAL export of this
// module, the driver found no local `run` to bind, and the namespace entry
// degraded to an undefined-returning stub — so `(await import(...)).run` was
// `undefined` and calling it returned `undefined` instead of throwing.
import { run, VERSION } from "./dynamic_import_chunk_shared.ts";

// Also exercise the import-rename shape: the binding in the shared chunk is
// `VERSION`, re-exported here under a different consumer-visible key.
export { run, VERSION as version };
