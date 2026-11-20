// Arrays that leave the code the facts see (DESIGN-p4l1 §5.4 fixture 7):
// a serializer, a callback that writes the array it walks, `Reflect.set`,
// `Object.assign`, a native call taking the array. Each is read in a loop
// before and after.

import * as path from "node:path";

function total(a: any[], n: number): string {
  let s = 0;
  let other = "";
  for (let i = 0; i < n; i++) {
    const k = i;
    const v = a[k];
    if (typeof v === "number") s += v;
    else other += String(v);
  }
  return s + "/" + other;
}

const json = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
console.log("json", JSON.stringify(json).length, total(json, 20));

// A callback that stores a string into the array being walked.
const walked: any[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
console.log("walked before", total(walked, 18));
walked.forEach((v, i) => {
  if (i === 3) walked[0] = "s";
});
console.log("walked after", total(walked, 18));

// Reflect.set at an index, at `length`, and past the end.
const reflected: any[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
Reflect.set(reflected, 1, "r");
Reflect.set(reflected, 25, 7);
console.log("reflected", total(reflected, 27), reflected.length);
Reflect.set(reflected, "length", 2);
console.log("reflected cut", total(reflected, 4), reflected.length);

// Object.assign copies indexed keys in.
const assigned: any[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
Object.assign(assigned, ["x"], { 20: 5 });
console.log("assigned", total(assigned, 22), assigned.length);

// A native call that takes the array.
const parts = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q"];
console.log("native", path.posix.join(...parts), total(parts as any[], 17));

// An array handed to a function the caller chooses at run time.
const handed = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
const ops: ((a: number[]) => void)[] = [
  (a) => {
    a.length = 4;
  },
  (a) => {
    a.reverse();
  },
];
console.log("handed before", total(handed, 18));
ops[process.argv.length > 100 ? 1 : 0](handed);
console.log("handed after", total(handed, 18), handed.length);
