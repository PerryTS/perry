// #2792 Array .with RangeError (array path; typed-array .with routing is a
//        separate pre-existing bug, see PR notes)
// #2814 zero + variadic Array.prototype.unshift
// #2794 omitted deleteCount for Array.prototype.toSpliced
// #2796 sort/toSorted comparator validation (Array + TypedArray)

function tryWith(arr: number[], idx: number): string {
  try {
    return JSON.stringify(arr.with(idx, 9));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}

// ---- #2792 Array.with ----
console.log("with:1=" + tryWith([1, 2, 3], 1));
console.log("with:-1=" + tryWith([1, 2, 3], -1));
console.log("with:3=" + tryWith([1, 2, 3], 3));
console.log("with:-4=" + tryWith([1, 2, 3], -4));
console.log("with:NaN=" + tryWith([1, 2, 3], NaN));
console.log("with:Inf=" + tryWith([1, 2, 3], Infinity));

// ---- #2814 unshift ----
const a = [1, 2];
console.log("unshift0=" + a.unshift());
console.log("unshift0arr=" + JSON.stringify(a));

const b = [3, 4];
console.log("unshift2=" + b.unshift(1, 2));
console.log("unshift2arr=" + JSON.stringify(b));

const c = [4];
console.log("unshift3=" + c.unshift(1, 2, 3));
console.log("unshift3arr=" + JSON.stringify(c));

const d = [5];
console.log("unshift1=" + d.unshift(0));
console.log("unshift1arr=" + JSON.stringify(d));

// ---- #2794 toSpliced ----
const base = [1, 2, 3, 4];
console.log("ts:noargs=" + JSON.stringify(base.toSpliced()));
console.log("ts:start=" + JSON.stringify(base.toSpliced(1)));
console.log("ts:undef=" + JSON.stringify(base.toSpliced(1, undefined)));
console.log("ts:del2=" + JSON.stringify(base.toSpliced(1, 2)));
console.log("ts:insdel=" + JSON.stringify(base.toSpliced(1, 2, 9, 10)));
console.log("ts:negstart=" + JSON.stringify(base.toSpliced(-1, 1, 9)));
console.log("ts:toonegstart=" + JSON.stringify(base.toSpliced(-10, 1, 9)));
console.log("ts:toolargestart=" + JSON.stringify(base.toSpliced(10, 1, 9)));
console.log("ts:infdel=" + JSON.stringify(base.toSpliced(1, Infinity, 9)));
console.log("ts:orig=" + JSON.stringify(base));

// ---- #2796 Array sort/toSorted comparator validation ----
function trySortJSON(fn: () => any): string {
  try {
    return JSON.stringify(fn());
  } catch (e: any) {
    return "throws " + e.name + ": " + e.message;
  }
}
console.log("Array.sort(undefined): " + trySortJSON(() => [3, 1, 10, 2].sort(undefined)));
console.log("Array.sort(null): " + trySortJSON(() => [3, 1, 10, 2].sort(null as any)));
console.log("Array.sort(1): " + trySortJSON(() => [3, 1, 10, 2].sort(1 as any)));
console.log("Array.toSorted(undefined): " + trySortJSON(() => [3, 1, 10, 2].toSorted(undefined)));
console.log("Array.toSorted(null): " + trySortJSON(() => [3, 1, 10, 2].toSorted(null as any)));
console.log("Array.toSorted(1): " + trySortJSON(() => [3, 1, 10, 2].toSorted(1 as any)));
console.log("Array.sort(cmp): " + trySortJSON(() => [3, 1, 10, 2].sort((x, y) => x - y)));

// ---- #2796 TypedArray sort/toSorted comparator validation ----
// Only the validation (throw vs no-throw) is asserted here — the sorted
// *values* go through a separate pre-existing typed-array materialization path.
function tryTAValidate(fn: () => any): string {
  try {
    fn();
    return "ok";
  } catch (e: any) {
    return "throws " + e.name + ": " + e.message;
  }
}
console.log("U8.sort(undefined): " + tryTAValidate(() => new Uint8Array([3, 1, 10, 2]).sort(undefined)));
console.log("U8.sort(null): " + tryTAValidate(() => new Uint8Array([3, 1, 10, 2]).sort(null as any)));
console.log("U8.sort(1): " + tryTAValidate(() => new Uint8Array([3, 1, 10, 2]).sort(1 as any)));
console.log("U8.toSorted(undefined): " + tryTAValidate(() => new Uint8Array([3, 1, 10, 2]).toSorted(undefined)));
console.log("U8.toSorted(null): " + tryTAValidate(() => new Uint8Array([3, 1, 10, 2]).toSorted(null as any)));
console.log("U8.toSorted(1): " + tryTAValidate(() => new Uint8Array([3, 1, 10, 2]).toSorted(1 as any)));
