// arr[arr.length] = v on a local array, for every kind of value: the array
// keeps its element order, length, holes and layout through numbers,
// short and heap strings, objects, mixes, growth and a collection.
function fill(values: any[]): any[] {
  const arr: any[] = [];
  for (let i = 0; i < values.length; i++) arr[arr.length] = values[i];
  return arr;
}
const heapStr = "a fairly long heap string that is not short";
const vals: any[] = [1, "a", heapStr, { k: 1 }, [2], null, undefined, true, 2.5, -0, NaN, 10n, Symbol.for("s"), "%41", heapStr + "!"];
const out = fill(vals);
console.log(out.length, out.map((v) => typeof v).join(","));
console.log(String(out[0]), out[1], out[2].length, out[3].k, out[4][0], out[5], out[6], out[7], out[8], Object.is(out[9], -0), out[10], out[11], out[12].toString(), out[13], out[14].length);
// Pointer-only, number-only and string-only arrays through growth.
const objs: any[] = [];
for (let i = 0; i < 300; i++) objs[objs.length] = { i };
const nums: number[] = [];
for (let i = 0; i < 300; i++) nums[nums.length] = i * 0.5;
const strs: string[] = [];
for (let i = 0; i < 300; i++) strs[strs.length] = "s" + i + (i % 7 === 0 ? heapStr : "");
console.log(objs.length, objs[299].i, nums.length, nums[299], strs.length, strs[0].length, strs[1], strs.join("").length);
// A unique (appendable) string stored, then appended to: the stored copy must not change.
let acc = "x";
for (let i = 0; i < 40; i++) acc += "y";
const keep: string[] = [];
keep[keep.length] = acc;
acc += "zzz";
console.log(keep[0].length, acc.length, keep[0].endsWith("y"));
// Survives a collection with young values stored into an old array.
const big: any[] = [];
for (let i = 0; i < 20000; i++) { big[big.length] = { v: i, s: "v" + i }; if (i % 5000 === 0) { const junk: any[] = []; for (let j = 0; j < 50000; j++) junk.push({ j }); } }
let sum = 0; for (let i = 0; i < big.length; i++) sum += big[i].v + big[i].s.length;
console.log(big.length, sum);
// Frozen and holey arrays, and an index that is not the length.
const frozen: any[] = Object.freeze([1, 2]) as any;
try { (function () { "use strict"; frozen[frozen.length] = 3; })(); } catch (e: any) { console.log("frozen", e instanceof TypeError); }
const holey: any[] = [];
holey[2] = "h";
holey[holey.length] = "next";
console.log(holey.length, JSON.stringify(holey));
