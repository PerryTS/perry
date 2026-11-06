import * as fs from "node:fs";

const toUnix = fs._toUnixTimestamp;
const { _toUnixTimestamp } = fs;

function codeOf(value: unknown): string {
  try {
    toUnix(value);
    return "NO_THROW";
  } catch (err) {
    return (err as { name?: string; code?: string }).name + ":" + (err as { code?: string }).code;
  }
}

const beforeNegative = Date.now() / 1000;
const negative = toUnix(-1);
const afterNegative = Date.now() / 1000;

console.log("key enumerable:", Object.keys(fs).indexOf("_toUnixTimestamp") !== -1);
console.log("typeof:", typeof toUnix);
console.log("name length:", toUnix.name, toUnix.length);
console.log("destructured same:", toUnix === _toUnixTimestamp);
console.log("number:", toUnix(1.5));
console.log("string numeric:", toUnix("2"));
console.log("string empty:", toUnix(""));
console.log("string spaces:", toUnix(" 3 "));
console.log("date seconds:", toUnix(new Date(1000)));
console.log("date fractional:", toUnix(new Date(1500)));
console.log("date invalid:", Number.isNaN(toUnix(new Date(NaN))));
console.log("negative current:", negative >= beforeNegative && negative <= afterNegative);
console.log("invalid undefined:", codeOf(undefined));
console.log("invalid null:", codeOf(null));
console.log("invalid bool:", codeOf(true));
console.log("invalid NaN:", codeOf(NaN));
console.log("invalid Infinity:", codeOf(Infinity));
console.log("invalid bad string:", codeOf("bad"));
console.log("invalid object:", codeOf({ valueOf: () => 4 }));
