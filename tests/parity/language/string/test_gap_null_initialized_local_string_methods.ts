// A local declared `null` (or `undefined`) and later assigned a string must
// dispatch the methods it shares with Array (`includes`, `indexOf`,
// `lastIndexOf`, `slice`) as string methods. Lowering used to read the
// initializer's `Null` type as "known not a string" and fold them to the
// array fast path (`false` / `-1`). three.js's LoopNode does
// `condition.includes("<")` on such a binding.
let late = null;
late = "a<b<";
console.log(late.includes("<"), late.indexOf("<"), late.lastIndexOf("<"), late.slice(1, 3));

let undef = undefined;
undef = "xyz";
console.log(undef.includes("y"), undef.indexOf("z"), undef.slice(-2));

function loopUpdate(start: number, end: number): string {
  let condition = null;
  if (start < end) {
    condition = "<";
  } else {
    condition = ">=";
  }
  return condition.includes("<") ? "++" : "--";
}
console.log(loopUpdate(0, 3), loopUpdate(3, 0));

// A real array behind the same declaration keeps working.
let arr = null;
arr = [1, 2, 3];
console.log(arr.includes(2), arr.indexOf(3), arr.slice(1).length);
