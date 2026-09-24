// #11244, cycle form: a synthesized import must not close an import cycle.
//
// cyc_lib.ts loads cyc_holder.ts lazily. The cross-module inliner harvested
// Holder.k(), whose body reads K from cyc_z.ts, and gave cyc_lib.ts (which
// never calls k()) a synthesized import of cyc_z.ts. That import is an init
// edge, and cyc_z.ts imports cyc_lib.ts back, so cyc_z's body ran before
// cyc_lib's and read `config` while it was still undefined:
//
//   TypeError: Cannot read properties of undefined (reading 'n')
//
// the same shape as mongodb 7.5.0's startup failure, where
// mongodb-connection-string-url's redact.js gained an edge to
// mongodb/src/index.ts and re-entered lib/index.js mid-init
// (`Cannot read properties of undefined (reading 'default')`).
import { config, loadHolder } from "./fixtures/issue_11244_inliner_import_edges/cyc_lib.ts";

console.log("main", config.n);

(async () => {
  console.log("holder", await loadHolder());
  console.log("done");
})();
