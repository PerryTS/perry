// Consumer for the anonymous-default-function regression. Imports the
// anonymous default function from `producer.ts` and invokes it. Expected
// output is `42` byte-for-byte (matches `node --experimental-strip-types`).
import x from "./producer.ts";
console.log(x());
