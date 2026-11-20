// Issue #836 fixture: barrel mirroring zod's `node_modules/zod/src/index.ts`.
// Shape under test:
//   import * as z from "./external.ts";
//   export { z };
// The `Export::Named { local: "z", exported: "z" }` entry was skipped by
// every wrapper-emission loop (`local==exported` and `z` is a namespace
// import, not a HIR function), so consumers that read `z` as a value
// link-failed on `__perry_wrap_perry_fn_<index_ts>__z`.

import * as z from "./external.ts";

export { z };
