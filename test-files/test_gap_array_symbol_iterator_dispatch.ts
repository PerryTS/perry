// #321 (effect Stream/Chunk): a dynamic `arr[Symbol.iterator]()` / `arr.values()`
// dispatch must materialize the iteration sequence. effect's `Chunk[Symbol.iterator]`
// does `this.backing.array[Symbol.iterator]()`, which resolves to the array's bound
// `values` method at runtime. Before the fix that bound-method dispatch fell through
// the array method tower and returned a sentinel, so every `for...of` over the result
// yielded nothing (and downstream `.next()`/`.values()` threw "values is not a
// function"). These cases exercise the runtime dispatch path (not codegen's static
// array inline path).

// 1) Direct symbol-keyed iterator call on a field-chain receiver (the Chunk shape).
const obj = { backing: { array: [1, 2, 3] } };
const it = obj.backing.array[Symbol.iterator]();
const out: number[] = [];
for (const x of it) {
  out.push(x as number);
}
console.log("field-chain iter:", JSON.stringify(out));

// 2) Spreading a symbol-keyed iterator result.
const spread = [...obj.backing.array[Symbol.iterator]()];
console.log("spread:", JSON.stringify(spread));

// 3) Dynamic `.values()` dispatch on an array via an any-typed binding.
const anyArr: any = [10, 20, 30];
const valsOut: number[] = [];
for (const v of anyArr.values()) {
  valsOut.push(v as number);
}
console.log("values:", JSON.stringify(valsOut));

// 4) Dynamic `.entries()` / `.keys()` dispatch on an array.
const entriesOut: Array<[number, number]> = [];
for (const e of anyArr.entries()) {
  entriesOut.push(e as [number, number]);
}
console.log("entries:", JSON.stringify(entriesOut));
const keysOut: number[] = [];
for (const k of anyArr.keys()) {
  keysOut.push(k as number);
}
console.log("keys:", JSON.stringify(keysOut));
