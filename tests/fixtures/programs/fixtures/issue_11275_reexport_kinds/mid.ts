// Named re-exports of IMPORTED bindings, plain and renamed.
import { obj, plainObj, num, str, list, fn, arrow, Cls, counter, bump } from "./origin.ts";
import * as originNs from "./origin.ts";
import cjs from "./lib.cjs";
export { obj, plainObj, num, str, list, fn, arrow, Cls, counter, bump };
export { obj as objAlias, fn as fnAlias, Cls as ClsAlias, arrow as arrowAlias };
export { originNs, cjs };
export * as starNs from "./origin.ts";
