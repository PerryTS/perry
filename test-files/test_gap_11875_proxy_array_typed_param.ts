// #11875: a Proxy whose target is an array, passed where the parameter is
// declared an array type (`number[]`, `Body[]`, `string[]`), must answer every
// index read through the proxy's `[[Get]]` (its `get` trap, else the target).
// The declared element type is a hint, never a layout fact: before the fix the
// counter-loop and straight-line element tiers read `undefined` for every
// element, and a counter loop over class elements dereferenced the proxy id as
// a heap header (SIGSEGV).

class Body {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

// numeric counter loop (packed-f64 tier on a real array)
function sumNums(bs: number[]): number {
  let s = 0;
  for (let i = 0; i < bs.length; i++) s += bs[i];
  return s;
}

// straight-line constant index
function firstX(bs: Body[]): number {
  return bs[0].x;
}

// straight-line variable index
function atX(bs: Body[], i: number): number {
  return bs[i].x;
}

// counter loop reading fields of each element (element-shape tier)
function sumBodies(bs: Body[]): number {
  let s = 0;
  for (let i = 0; i < bs.length; i++) {
    const b = bs[i];
    s += b.x * 2 + b.y;
  }
  return s;
}

// field read straight off the index expression
function sumX(bs: Body[]): number {
  let s = 0;
  for (let i = 0; i < bs.length; i++) s += bs[i].x;
  return s;
}

// reverse and while loops
function sumReverse(bs: number[]): number {
  let s = 0;
  for (let i = bs.length - 1; i >= 0; i--) s += bs[i];
  return s;
}
function sumWhile(bs: number[]): number {
  let s = 0;
  let i = 0;
  while (i < bs.length) {
    s += bs[i];
    i++;
  }
  return s;
}

// string elements
function joinAll(xs: string[]): string {
  let out = "";
  for (let i = 0; i < xs.length; i++) out += xs[i] + ";";
  return out;
}

// out-of-range read through the proxy
function past(bs: number[]): string {
  return String(bs[bs.length]);
}

// a member declared as an array
class Holder {
  items: number[];
  constructor(items: number[]) {
    this.items = items;
  }
  total(): number {
    let s = 0;
    for (let i = 0; i < this.items.length; i++) s += this.items[i];
    return s;
  }
}

const nums = [1, 2, 3];
const bodies = [new Body(1, 2), new Body(3, 4), new Body(5, 6)];

console.log("sumNums array", sumNums(nums));
console.log("sumNums proxy", sumNums(new Proxy(nums, {})));
console.log("firstX array", firstX(bodies));
console.log("firstX proxy", firstX(new Proxy(bodies, {})));
console.log("atX proxy", atX(new Proxy(bodies, {}), 2));
console.log("sumBodies array", sumBodies(bodies));
console.log("sumBodies proxy", sumBodies(new Proxy(bodies, {})));
console.log("sumBodies array again", sumBodies(bodies));
console.log("sumX proxy", sumX(new Proxy(bodies, {})));
console.log("sumReverse proxy", sumReverse(new Proxy(nums, {})));
console.log("sumWhile proxy", sumWhile(new Proxy(nums, {})));
console.log("joinAll proxy", joinAll(new Proxy(["a", "b", "c"], {})));
console.log("past proxy", past(new Proxy(nums, {})));
console.log("Holder array", new Holder(nums).total());
console.log("Holder proxy", new Holder(new Proxy(nums, {})).total());

// the get trap sees every element read, with the canonical string key
const seen: string[] = [];
const traced = new Proxy([10, 20, 30], {
  get(target: any, key: any, recv: any) {
    if (typeof key === "string" && key !== "length") seen.push(key);
    const v = Reflect.get(target, key, recv);
    return typeof v === "number" ? v * 2 : v;
  },
});
console.log("trap sumNums", sumNums(traced), seen.join(","));

const tracedBodies = new Proxy(bodies, {
  get(target: any, key: any, recv: any) {
    if (key === "1") return new Body(100, 200);
    return Reflect.get(target, key, recv);
  },
});
console.log("trap sumBodies", sumBodies(tracedBodies));
console.log("trap firstX/atX", firstX(tracedBodies), atX(tracedBodies, 1));
