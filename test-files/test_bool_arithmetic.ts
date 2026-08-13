// Test boolean values in arithmetic and coercion contexts
// Critical for static typing migration: booleans must coerce to 0/1 in arithmetic

// === Comparison results in arithmetic ===
let count = 0;
count += 1;
count += 1;
count += 1;
console.log(count); // 3

// === Boolean in ternary ===
const x = 5;
const y = 3;
const bigger = x > y ? "yes" : "no";
console.log(bigger); // yes

// === Boolean in if ===
if (10 > 5) {
  console.log("10 > 5"); // 10 > 5
}

if (!(3 > 7)) {
  console.log("3 not > 7"); // 3 not > 7
}

// === Boolean variables ===
const t: boolean = true;
const f: boolean = false;
console.log(t);  // true
console.log(f);  // false

// === Statically proven booleans in numeric operators ===
// These remain native i1 -> f64 operations in codegen (#5497 Lever E).
console.log(t + 2);  // 3
console.log(f * 9);  // 0
console.log(t < 2);  // true
console.log(f >= 1); // false

// Type annotations are optimization hints, not runtime guards. Invalidating a
// proven boolean through `any` must retain dynamic + / ToNumber semantics.
let maybeBool: boolean = true;
maybeBool = "4" as any;
console.log(maybeBool + 2); // 42
console.log(maybeBool * 2); // 8
console.log(maybeBool > 2); // true

// === Boolean in while loop ===
let i = 0;
let flag = true;
while (flag) {
  i++;
  if (i >= 3) flag = false;
}
console.log(i); // 3

// === Negation ===
console.log(!true);   // false
console.log(!false);  // true
console.log(!!0);     // false
console.log(!!1);     // true
console.log(!!"");    // false
console.log(!!"hi");  // true

// === Boolean comparison chains ===
const a = 10;
const b = 20;
const c = 15;
console.log(a < c && c < b);  // true
console.log(a > c || c > b);  // false

// === Boolean coercion to string ===
const boolStr = "result: " + (5 > 3);
console.log(boolStr); // result: true

const boolStr2 = "negative: " + (5 < 3);
console.log(boolStr2); // negative: false

// === Equality operators returning booleans ===
console.log(1 === 1);   // true
console.log(1 === 2);   // false
console.log(1 !== 2);   // true
console.log("a" === "a"); // true
console.log("a" === "b"); // false
console.log("a" !== "b"); // true

// === Boolean in array ===
const bools: boolean[] = [true, false, true];
console.log(bools[0]); // true
console.log(bools[1]); // false
console.log(bools.length); // 3
