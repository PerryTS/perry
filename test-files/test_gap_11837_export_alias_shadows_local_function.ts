// #11837: prettier's `format()` resolved to undefined. index.mjs has
// `async function formatWithCursor` and exports its wrapper as
// `export { formatWithCursor2 as formatWithCursor }`. The export's getter took
// the symbol `perry_fn_<mod>__formatWithCursor`, which the local function
// also mangles to, so the getter replaced the local function's body:
// `withPlugins(formatWithCursor)` wrapped the getter, and calling the export
// returned the wrapper itself instead of a Promise of the result.
import * as lib from "./gap_11837_export_alias_helper.mjs";
import { inner, second, $i, helper } from "./gap_11837_export_alias_helper.mjs";

const r: any = await lib.formatWithCursor("a", { suffix: "!" });
console.log("formatWithCursor", typeof r, r && r.formatted);
console.log("inner", lib.inner("x"), inner("y"));
console.log("second", lib.second(), second());
console.log("$i", lib.$i(), $i());
console.log("helper", helper());
console.log("local calls", lib.localCalls());
