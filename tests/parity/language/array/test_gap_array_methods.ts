// Gap test: Array methods not yet supported by Perry
// Run: node --experimental-strip-types test_gap_array_methods.ts

// --- Array.of ---
const a1 = Array.of(1, 2, 3);
console.log("Array.of(1,2,3):", a1); // [1, 2, 3]
console.log("Array.of length:", a1.length); // 3

const a2 = Array.of("a", "b");
console.log("Array.of strings:", a2); // ['a', 'b']

// --- arr.at() (positive and negative indexing) ---
const arr = [10, 20, 30, 40, 50];
console.log("at(0):", arr.at(0)); // 10
console.log("at(2):", arr.at(2)); // 30
console.log("at(-1):", arr.at(-1)); // 50
console.log("at(-2):", arr.at(-2)); // 40
console.log("at(10):", arr.at(10)); // undefined

// --- arr.findLast ---
const nums = [1, 2, 3, 4, 5, 6];
const lastEven = nums.findLast((x: number) => x % 2 === 0);
console.log("findLast even:", lastEven); // 6

const lastOver10 = nums.findLast((x: number) => x > 10);
console.log("findLast >10:", lastOver10); // undefined

// --- arr.findLastIndex ---
const lastEvenIdx = nums.findLastIndex((x: number) => x % 2 === 0);
console.log("findLastIndex even:", lastEvenIdx); // 5

const lastOver10Idx = nums.findLastIndex((x: number) => x > 10);
console.log("findLastIndex >10:", lastOver10Idx); // -1

// --- arr.toReversed() (immutable) ---
const original = [1, 2, 3, 4, 5];
const reversed = original.toReversed();
console.log("toReversed:", reversed); // [5, 4, 3, 2, 1]
console.log("original unchanged:", original); // [1, 2, 3, 4, 5]

// --- arr.toSorted() (immutable) ---
const unsorted = [3, 1, 4, 1, 5, 9, 2, 6];
const sorted1 = unsorted.toSorted();
console.log("toSorted default:", sorted1); // [1, 1, 2, 3, 4, 5, 6, 9]
console.log("original unchanged:", unsorted); // [3, 1, 4, 1, 5, 9, 2, 6]

const sorted2 = unsorted.toSorted((a: number, b: number) => b - a);
console.log("toSorted descending:", sorted2); // [9, 6, 5, 4, 3, 2, 1, 1]

// --- arr.toSpliced() (immutable) ---
const base = [1, 2, 3, 4, 5];
const spliced1 = base.toSpliced(1, 2);
console.log("toSpliced(1,2):", spliced1); // [1, 4, 5]
console.log("original unchanged:", base); // [1, 2, 3, 4, 5]

const spliced2 = base.toSpliced(1, 2, 20, 30);
console.log("toSpliced(1,2,20,30):", spliced2); // [1, 20, 30, 4, 5]

const spliced3 = base.toSpliced(0, 0, 99);
console.log("toSpliced insert at 0:", spliced3); // [99, 1, 2, 3, 4, 5]

// --- arr.with() (immutable set) ---
const withArr = [10, 20, 30, 40];
const replaced = withArr.with(1, 99);
console.log("with(1, 99):", replaced); // [10, 99, 30, 40]
console.log("original unchanged:", withArr); // [10, 20, 30, 40]

const replacedNeg = withArr.with(-1, 0);
console.log("with(-1, 0):", replacedNeg); // [10, 20, 30, 0]

// --- arr.copyWithin ---
const cw1 = [1, 2, 3, 4, 5];
cw1.copyWithin(0, 3);
console.log("copyWithin(0,3):", cw1); // [4, 5, 3, 4, 5]

const cw2 = [1, 2, 3, 4, 5];
cw2.copyWithin(1, 0, 2);
console.log("copyWithin(1,0,2):", cw2); // [1, 1, 2, 4, 5]

// --- arr.reduceRight ---
const rr1 = [1, 2, 3, 4].reduceRight((acc: number, val: number) => acc + val, 0);
console.log("reduceRight sum:", rr1); // 10

const rr2 = ["a", "b", "c"].reduceRight((acc: string, val: string) => acc + val, "");
console.log("reduceRight concat:", rr2); // "cba"

const rr3 = [[1, 2], [3, 4], [5, 6]].reduceRight(
  (acc: number[], val: number[]) => acc.concat(val),
  [] as number[]
);
console.log("reduceRight flatten:", rr3); // [5, 6, 3, 4, 1, 2]

// --- arr.entries(), arr.keys(), arr.values() as iterators ---
const iterArr = ["a", "b", "c"];

const entriesResult: [number, string][] = [];
for (const entry of iterArr.entries()) {
  entriesResult.push(entry);
}
console.log("entries():", entriesResult); // [[0,'a'], [1,'b'], [2,'c']]

const keysResult: number[] = [];
for (const key of iterArr.keys()) {
  keysResult.push(key);
}
console.log("keys():", keysResult); // [0, 1, 2]

const valuesResult: string[] = [];
for (const val of iterArr.values()) {
  valuesResult.push(val);
}
console.log("values():", valuesResult); // ['a', 'b', 'c']

// --- Object.groupBy (Node 22+) ---
if (typeof Object.groupBy === "function") {
  const items = [
    { name: "apple", type: "fruit" },
    { name: "carrot", type: "vegetable" },
    { name: "banana", type: "fruit" },
    { name: "broccoli", type: "vegetable" },
  ];
  const grouped = Object.groupBy(items, (item: { name: string; type: string }) => item.type);
  console.log("Object.groupBy fruit:", grouped.fruit?.map((i: { name: string }) => i.name)); // ['apple', 'banana']
  console.log("Object.groupBy vegetable:", grouped.vegetable?.map((i: { name: string }) => i.name)); // ['carrot', 'broccoli']
} else {
  console.log("Object.groupBy: not available");
}

// --- Array.fromAsync (Node 22+) ---
async function testFromAsync() {
  if (typeof Array.fromAsync === "function") {
    async function* gen() {
      yield 1;
      yield 2;
      yield 3;
    }
    const result = await Array.fromAsync(gen());
    console.log("Array.fromAsync:", result); // [1, 2, 3]

    const fromPromises = await Array.fromAsync([
      Promise.resolve(10),
      Promise.resolve(20),
      Promise.resolve(30),
    ]);
    console.log("Array.fromAsync promises:", fromPromises); // [10, 20, 30]
  } else {
    console.log("Array.fromAsync: not available");
  }
}

// --- Typed Arrays ---
const i32 = new Int32Array([1, 2, 3, 4, 5]);
console.log("Int32Array:", i32); // Int32Array [1, 2, 3, 4, 5]
console.log("Int32Array.at(0):", i32.at(0)); // 1
console.log("Int32Array.at(-1):", i32.at(-1)); // 5
console.log("Int32Array length:", i32.length); // 5

const f64 = new Float64Array([1.1, 2.2, 3.3]);
console.log("Float64Array:", f64); // Float64Array [1.1, 2.2, 3.3]
console.log("Float64Array.at(-1):", f64.at(-1)); // 3.3

const u8 = new Uint8Array([255, 0, 128]);
console.log("Uint8Array:", u8); // Uint8Array [255, 0, 128]
console.log("Uint8Array.at(1):", u8.at(1)); // 0

// Typed array methods
const ta = new Int32Array([5, 3, 1, 4, 2]);
const taSorted = ta.toSorted();
console.log("Int32Array.toSorted:", taSorted); // Int32Array [1, 2, 3, 4, 5]
console.log("original unchanged:", ta); // Int32Array [5, 3, 1, 4, 2]

const taReversed = ta.toReversed();
console.log("Int32Array.toReversed:", taReversed); // Int32Array [2, 4, 1, 3, 5]

const taWith = new Int32Array([10, 20, 30]);
const taReplaced = taWith.with(1, 99);
console.log("Int32Array.with(1,99):", taReplaced); // Int32Array [10, 99, 30]

// Typed array findLast / findLastIndex
const taFind = new Int32Array([1, 2, 3, 4, 5]);
console.log("Int32Array.findLast even:", taFind.findLast((x: number) => x % 2 === 0)); // 4
console.log("Int32Array.findLastIndex even:", taFind.findLastIndex((x: number) => x % 2 === 0)); // 3

// Run async tests
testFromAsync().then(() => {
  console.log("All array gap tests complete.");
});
