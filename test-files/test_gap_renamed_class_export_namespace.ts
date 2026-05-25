// epic #1785 / #1758: namespace-member access of a RENAMED class export.
//
// `export { Number$ as Number }` (a local renamed export) read via
// `import * as M from "..."; M.Number` resolved `class_ids["Number"]` (the
// export ALIAS) — a miss, since the class is named `Number$` — so `M.Number`
// fell back to the JS global `Number` (or undefined), and `M.Number.ast`
// (an inherited static field) read undefined.
//
// This blocked effect Schema DECODE: `S.Number = class Number$ extends
// make(numberKeyword) {}` re-exported as `Number`; `S.decodeUnknownSync(S.Number)`
// read `S.Number.ast` === undefined → `_tag` of undefined in ParseResult.
//
// Fix: record the origin (local) name for local renamed CLASS exports
// (compile.rs) and fall back to it when resolving a namespace-member class
// ref (property_get.rs).
//
// Compared byte-for-byte against `node --experimental-strip-types`.

import * as M from "./_helpers/renamed_class_export.ts";

// (1) renamed export whose alias collides with the global `Number`.
console.log("(1) M.Number.ast._tag:", (M as any).Number.ast?._tag);

// (2) renamed export, no global collision.
console.log("(2) M.Widget.ast._tag:", (M as any).Widget.ast?._tag);

// (3) direct export (regression guard).
console.log("(3) M.DirectCls.ast._tag:", (M as any).DirectCls.ast?._tag);

// (4) the global `Number` is still itself (not shadowed by the import).
console.log("(4) global Number(42):", Number("42"));
