// #2514 — util.setTraceSigInt(enable): boolean → undefined; non-boolean throws.
import { setTraceSigInt } from "node:util";

console.log(typeof setTraceSigInt);
console.log(String(setTraceSigInt(true)));
console.log(String(setTraceSigInt(false)));
try {
  setTraceSigInt("x" as unknown as boolean);
} catch (e) {
  console.log("badtype=" + (e as { code?: string }).code);
}
try {
  setTraceSigInt(1 as unknown as boolean);
} catch (e) {
  console.log("badnum=" + (e as { code?: string }).code);
}
