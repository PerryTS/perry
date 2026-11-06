import * as prefixed from "node:v8";
import legacy from "v8";
import { deserialize, serialize } from "node:v8";

console.log("module identity:", prefixed.serialize === legacy.serialize);
console.log(
  "named identity:",
  serialize === prefixed.serialize,
  deserialize === prefixed.deserialize,
);
console.log(
  "default keys:",
  Object.keys(legacy).length === Object.keys(prefixed).length,
);
console.log("roundtrip:", deserialize(serialize("identity")));
