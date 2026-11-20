import cjsDefault, * as namespace from "./fixtures/cjs-object.cjs";
import { named, shared } from "./fixtures/cjs-object.cjs";

console.log(
  "values:",
  cjsDefault.named,
  named,
  namespace.named,
  namespace.extra,
);
console.log(
  "identity:",
  namespace.default === cjsDefault,
  namespace.shared === shared,
  shared === cjsDefault.shared,
);
console.log("keys:", Object.keys(namespace).sort().join(","));
console.log(
  "module exports marker:",
  (namespace as any)["module.exports"] === cjsDefault,
);
console.log(
  "namespace tag/extensible:",
  Object.prototype.toString.call(namespace),
  Object.isExtensible(namespace),
);
