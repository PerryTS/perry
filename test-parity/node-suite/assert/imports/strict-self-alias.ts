import assertParent, { strict as parentStrict } from "node:assert";
import strictDefault, * as strictNs from "node:assert/strict";
import { strict } from "node:assert/strict";

const strictKeys = Object.keys(strictDefault);

console.log("keys has strict:", strictKeys.includes("strict"));
console.log("default strict type:", typeof strictDefault.strict);
console.log("default strict self:", strictDefault.strict === strictDefault);
console.log("parent strict default:", assertParent.strict === strictDefault);
console.log("parent named strict:", parentStrict === strictDefault);
console.log("named strict default:", strict === strictDefault);
console.log("namespace strict default:", strictNs.strict === strictDefault);
strict(true);
try {
  strict(false, "strict alias fail");
} catch (err) {
  console.log("strict fail name:", (err as Error).name);
}
console.log("strict call ok");
