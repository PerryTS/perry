// Issue #35 / #321: same-named functions exported from DIFFERENT modules,
// imported under distinct aliases, must each dispatch into their OWN module's
// body / signature.
//
// effect's Context/Layer DI hit this: helper functions with identical names
// live in separate modules. Perry resolved imported functions (and their
// signatures) by BARE NAME — two modules both exporting `equals` collided.
// The HIR lowered an aliased named import `{ equals as eqA }` to
// `ExternFuncRef { name: "equals" }` (the EXPORTED name), so both `eqA(...)`
// and `eqB(...)` carried the same name and the codegen's flat
// `import_function_prefixes` lookup keyed on "equals" picked whichever module's
// prefix landed last in the HashMap — BOTH calls dispatched into one module's
// body with the wrong signature.
//
// Fix (mirrors the #901 Default-import fix): a non-native named import now
// registers `(local, local)` so its `ExternFuncRef` carries the UNIQUE local
// name, and the CLI maps `local -> exported_name` in
// `import_function_origin_names` so the emitted symbol stays
// `perry_fn_<src>__<exported>`. Compared byte-for-byte against
// `node --experimental-strip-types`.
import { equals as eqA } from "./_helpers/same_name_xmodule_fn_a.ts";
import { equals as eqB } from "./_helpers/same_name_xmodule_fn_b.ts";

// eqA reads `arguments` (zero declared params); eqB uses two normal params.
console.log("eqA:", eqA("P", "Q")); // A:P,Q (len=2)
console.log("eqB:", eqB(3, 4)); // B:7

// Call again in the reverse order to make sure neither dispatch got rebound.
console.log("eqB2:", eqB(10, 20)); // B:30
console.log("eqA2:", eqA("X", "Y", "Z")); // A:X,Y (len=3)
