// Barrel re-export -- a THIRD view of the same bindings, once removed from
// the declaring module.
export {
  fnDecl,
  fnExpr,
  arrowFn,
  isSameDecl,
  isSameExpr,
  isSameArrow,
  bothSame,
  makeSet,
} from "./lib.ts";
export { default as useDeclDefault } from "./lib.ts";
