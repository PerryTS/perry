import * as bare from "async_hooks";
import * as prefixed from "node:async_hooks";

console.log(
  "namespace tags:",
  Object.prototype.toString.call(bare),
  Object.prototype.toString.call(prefixed),
);
console.log(
  "namespace toStringTag:",
  bare[Symbol.toStringTag],
  prefixed[Symbol.toStringTag],
);
