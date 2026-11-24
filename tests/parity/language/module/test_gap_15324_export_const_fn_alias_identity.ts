// #15324: `export const addRef = add` exports the function `add` itself, so
// every importer sees one function object under both names.
import { add, addRef, addLet, getAdd } from "./fixtures/export_const_fn_alias/lib.ts";
import * as lib from "./fixtures/export_const_fn_alias/lib.ts";
import { mulRef, getMul } from "./fixtures/export_const_fn_alias/private.ts";

console.log("named:", add === addRef, add === addLet, addRef === getAdd());
console.log("namespace:", lib.add === lib.addRef, lib.addRef === addRef);
(addRef as any).tag = "shared";
console.log("expando:", (add as any).tag, (getAdd() as any).tag);
console.log("calls:", addRef(1, 2), addLet(2, 3), lib.addRef(3, 4));
console.log("private:", mulRef === getMul(), mulRef(3, 4));
