// A for-in body that writes its receiver under a key made from the loop key
// (`o[k + "x"]`): each key the body makes names a new key of the receiver.
// The compile ends (the facts solve ran a per-key run for "a", then "ax",
// "axx", ... without end) and the loop visits only the keys present at its
// start (ECMA-262 14.7.5.9 EnumerateObjectProperties).
const o1: Record<string, number> = { a: 1 };
for (const k in o1) {
  o1[k + "x"] = 1;
}
console.log("suffix " + Object.keys(o1).join(","));

const o2: Record<string, unknown> = { a: 1 };
o2.b = 2;
o2.c = 3;
const read: unknown[] = [];
for (const k in o2) {
  o2["n" + k] = k;
  read.push(o2[k]);
}
console.log("prefix " + read.join(",") + " | " + Object.keys(o2).join(","));

const o3: Record<string, number> = { u7a: 1, u7b: 2 };
let sum = 0;
for (const k in o3) {
  sum += o3[k];
  o3[k + "x"] = 10;
}
console.log("sum " + sum + " | " + Object.keys(o3).join(","));

let s = "q";
const o4: Record<string, number> = { a: 1, b: 2 };
for (const k in o4) {
  s = k + s;
  o4[s] = 0;
}
console.log("chain " + Object.keys(o4).join(","));
