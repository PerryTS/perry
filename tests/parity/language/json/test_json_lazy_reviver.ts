// Regression for PERRY_JSON_TAPE=1: lazy top-level arrays must still run
// JSON.parse revivers for every element, not only for the root array.

let blob = "[";
for (let i = 0; i < 300; i++) {
  if (i > 0) {
    blob += ",";
  }
  blob += i;
}
blob += "]";

let calls = 0;
const parsed = JSON.parse(blob, (_key, value) => {
  calls += 1;
  if (typeof value === "number") {
    return value + 1;
  }
  return value;
}) as number[];

console.log("len", parsed.length);
console.log("first", parsed[0]);
console.log("last", parsed[299]);
console.log("calls", calls);
