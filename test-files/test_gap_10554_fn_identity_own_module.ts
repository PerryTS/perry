// #10554: a function referenced inside its own module is a different
// object from the same function imported elsewhere.
import useDecl, {
  fnDecl,
  fnExpr,
  arrowFn,
  isSameDecl,
  isSameExpr,
  isSameArrow,
  bothSame,
  makeSet,
} from "./_helpers/fn_identity_10554/lib.ts";
import * as ns from "./_helpers/fn_identity_10554/lib.ts";
import {
  fnDecl as reFnDecl,
  isSameDecl as reIsSameDecl,
  useDeclDefault,
} from "./_helpers/fn_identity_10554/reexport.ts";

// 1. function declaration, function expression, arrow: in-module identity
// checked from a call made through the IMPORTED value.
console.log("decl:", isSameDecl(fnDecl));
console.log("expr:", isSameExpr(fnExpr));
console.log("arrow:", isSameArrow(arrowFn));

// 2. Both directions: importer's namespace view vs named-import view vs
// in-module (through the exported checker functions).
console.log("ns decl:", isSameDecl(ns.fnDecl), ns.fnDecl === fnDecl, fnDecl === ns.fnDecl);
console.log("ns expr:", isSameExpr(ns.fnExpr), ns.fnExpr === fnExpr);
console.log("ns arrow:", isSameArrow(ns.arrowFn), ns.arrowFn === arrowFn);

// 3. Re-export (barrel): a third view, once removed.
console.log("reexport decl:", reIsSameDecl(reFnDecl), reFnDecl === fnDecl, isSameDecl(reFnDecl));

// 4. default export whose body ALSO references an exported sibling by
// value (distinct from #10434/#10548's export-row identity; exercises the
// cross-module-inliner defect instead).
console.log("default:", useDecl(fnDecl), useDeclDefault(fnDecl), useDecl === useDeclDefault);

// 5. bothSame -- direct pass-through, no local materialization inside the
// callee (a control: unaffected by this defect, should always have passed).
console.log("bothSame decl:", bothSame(fnDecl, fnDecl), bothSame(fnDecl, ns.fnDecl));
console.log("bothSame cross:", bothSame(fnDecl, fnExpr));

// 6. Set membership -- identity through collection storage/lookup, built
// FROM WITHIN the module (the same defect shape via a different value
// consumer than `===`).
const set = makeSet();
console.log("set has:", set.has(fnDecl), set.has(fnExpr), set.has(arrowFn));
console.log("set has via ns:", set.has(ns.fnDecl));
