// Reuse the result object to expose next/done/value reads without allocating
// a result or retaining a large array on every iteration.
const count = Number(process.argv[2] || "200000");
let index = 0;
const result = { done: false, value: 0 };
const iterator: any = {
  next() {
    result.done = index >= count;
    result.value = index++;
    return result;
  },
};
const iterable: any = { [Symbol.iterator]() { return iterator; } };
let sum = 0;
for (const value of iterable) sum += value;
if (sum !== count * (count - 1) / 2) throw new Error("iterator mismatch");
console.log(sum);
