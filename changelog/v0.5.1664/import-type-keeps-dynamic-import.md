An `import type` of a module no longer drops a real `import()` or Worker of
the same module in the same file.

`collect_modules` folds a dynamic edge (an `import("./x.ts")`, or a Worker
target found at a `new Worker(new URL("./x.ts", import.meta.url))` site) into an
existing static import of the same source, marking it `is_dynamic_target`. It
picked any static import, including an erased one: `import type { X } from
"./x.ts"` (`type_only`) or `import { type X } from "./x.ts"`
(`runtime_erased`). A `type_only` target is never queued for compilation, so the
dynamic import rejected at run time with "Cannot find module", and the Worker
form failed to compile with "worker_threads Worker target was not compiled".
upm's `store.ts` hit this on every run: its `import type { Pool, Sink } from
"./unpack-pool.ts"` dropped the `import("./unpack-pool.ts")` that loads the
unpack pool, so upm always unpacked on the main thread.

A dynamic edge is now folded only into a static edge that exists at run time;
otherwise it gets its own dynamic edge.

Tests: `test_gap_import_type_dynamic_import`, `test_gap_worker_type_import_edge`.

Merge after the namespace Worker constructor change
(`worker-builtin-module-ctor`): once upm's unpack pool loads, it constructs its
Workers through `process.getBuiltinModule("node:worker_threads").Worker`.
