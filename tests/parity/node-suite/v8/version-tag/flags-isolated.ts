import { cachedDataVersionTag, setFlagsFromString } from "node:v8";

const before = cachedDataVersionTag();
console.log("return:", setFlagsFromString("--allow_natives_syntax"));
const after = cachedDataVersionTag();
console.log("changed:", before !== after);
console.log("stable after:", after === cachedDataVersionTag());

for (const value of [undefined, 1] as const) {
  try {
    setFlagsFromString(value as any);
    console.log("invalid " + String(value) + ": no throw");
  } catch (error: any) {
    console.log("invalid " + String(value) + ":", error.name, error.code);
  }
}
