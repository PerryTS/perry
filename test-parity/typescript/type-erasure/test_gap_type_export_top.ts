// WPB4d: a TS type-only export (`export type`, `export interface`) is erased at run time: the
// module namespace has no such key. The facts modeled each one as an export whose value is ⊤
// ("unresolved scope name Color"), so the namespace's aggregate view held ⊤ and every computed
// read of the namespace was unproven. Node and the compiled program must agree on the keys and
// on the class read through a computed key.
import * as E from "./test_gap_type_export_top_mod.ts";
const name = process.argv[2] ?? "Chess";
const C = (E as any)[name];
const c = new C();
console.log(c.v + c.w, c.color, c.piece().v);
console.log(Object.keys(E).sort().join(","));
console.log("Color" in E, "Chess" in E, typeof (E as any).Piece);
console.log(E.make().w, E.START);
