// Iterator helpers as first-class `Iterator.prototype` methods (ES2025
// §27.1.4): reflective calls, metadata, argument validation that closes the
// underlying iterator, the callback counter, and overrides being observed.

function errName(f: () => unknown): string {
  try {
    f();
    return "no error";
  } catch (e: any) {
    return e && e.constructor ? e.constructor.name : String(e);
  }
}

// A plain `{ next, return }` iterator that records what the helpers did to it.
function plain(n: number, log: string[]): any {
  let i = 0;
  return {
    next() {
      log.push("next");
      return i < n ? { value: i++, done: false } : { value: undefined, done: true };
    },
    return() {
      log.push("return");
      return { value: undefined, done: true };
    },
  };
}

function* gen() {
  yield 1;
  yield 2;
  yield 3;
}

const P: any = Iterator.prototype;
const names = ["map", "filter", "take", "drop", "flatMap", "reduce", "toArray", "forEach", "some", "every", "find"];

// --- metadata ---
for (const name of names) {
  const f = P[name];
  const d = Object.getOwnPropertyDescriptor(P, name)!;
  console.log("meta", name, typeof f, f.name, f.length, d.writable, d.enumerable, d.configurable);
}

// --- reflective calls on a plain iterator object ---
{
  const log: string[] = [];
  const mapped = P.map.call(plain(3, log), (x: number, i: number) => `${x}@${i}`);
  console.log("map.call", JSON.stringify(mapped.toArray()), log.join(","));
  console.log("helper proto", Object.getPrototypeOf(Object.getPrototypeOf(mapped)) === P);
}
console.log("toArray.call", JSON.stringify(P.toArray.call(plain(2, []))));
console.log("reduce.call", P.reduce.call(plain(4, []), (a: number, b: number) => a + b));

// --- counter argument ---
{
  const seen: string[] = [];
  gen().filter((v: number, i: number) => (seen.push(`f${v}:${i}`), true)).toArray();
  gen().forEach((v: number, i: number) => seen.push(`e${v}:${i}`));
  gen().some((v: number, i: number) => (seen.push(`s${v}:${i}`), false));
  gen().every((v: number, i: number) => (seen.push(`v${v}:${i}`), true));
  gen().find((v: number, i: number) => (seen.push(`d${v}:${i}`), false));
  gen().flatMap((v: number, i: number) => (seen.push(`m${v}:${i}`), [v])).toArray();
  gen().reduce((a: number, v: number, i: number) => (seen.push(`r${v}:${i}`), a + v));
  gen().reduce((a: number, v: number, i: number) => (seen.push(`R${v}:${i}`), a + v), 0);
  console.log("counter", seen.join(" "));
}

// --- bad `this` ---
for (const name of names) {
  console.log("this", name, errName(() => P[name].call(1, () => 0)), errName(() => P[name].call(undefined, () => 0)));
}

// --- non-callable callback: TypeError after closing the underlying iterator ---
for (const name of ["map", "filter", "flatMap", "forEach", "some", "every", "find", "reduce"]) {
  const log: string[] = [];
  console.log("callable", name, errName(() => P[name].call(plain(2, log), 42)), log.join(","));
}

// --- take/drop limit validation ---
for (const name of ["take", "drop"]) {
  for (const limit of [NaN, -1, "x", undefined]) {
    const log: string[] = [];
    console.log("limit", name, String(limit), errName(() => P[name].call(plain(2, log), limit)), log.join(","));
  }
  const log: string[] = [];
  console.log("limit", name, "Infinity", JSON.stringify(P[name].call(plain(2, log), Infinity).toArray()));
  let calls = 0;
  const lim = { valueOf() { calls++; return 1; } };
  console.log("limit", name, "valueOf", JSON.stringify(P[name].call(plain(3, []), lim).toArray()), calls);
}
{
  const log: string[] = [];
  const t = P.take.call(plain(5, log), 2);
  console.log("take", JSON.stringify(t.toArray()), log.join(","));
}

// --- reduce on an empty iterator without an initial value ---
console.log("reduce empty", errName(() => [].values().reduce((a: number, b: number) => a + b)));
console.log("reduce empty init", [].values().reduce((a: number, b: number) => a + b, 7));

// --- flatMap rejects strings, accepts iterables and iterators ---
console.log("flatMap string", errName(() => gen().flatMap(() => "ab" as any).toArray()));
console.log("flatMap number", errName(() => gen().flatMap(() => 5 as any).toArray()));
console.log("flatMap ok", JSON.stringify(gen().flatMap((x: number) => (x === 2 ? new Set([x, -x]) : [x])).toArray()));

// --- helper.return() closes the underlying iterator ---
{
  const log: string[] = [];
  const h = P.map.call(plain(3, log), (x: number) => x);
  console.log("return start", JSON.stringify(h.return()), log.join(","));
  console.log("after return", JSON.stringify(h.next()), log.join(","));
}
{
  const log: string[] = [];
  const h = P.filter.call(plain(3, log), () => true);
  h.next();
  console.log("return yield", JSON.stringify(h.return()), log.join(","));
  console.log("return twice", JSON.stringify(h.return()), log.join(","));
}
{
  const log: string[] = [];
  const h = P.take.call(plain(3, log), 1);
  for (const v of h) log.push(`v${v}`);
  console.log("for-of take", log.join(","));
}

// --- callback throws: the underlying iterator is closed, the error propagates ---
{
  const log: string[] = [];
  const h = P.map.call(plain(3, log), () => {
    throw new RangeError("boom");
  });
  console.log("cb throws", errName(() => h.next()), log.join(","));
  console.log("cb throws done", JSON.stringify(h.next()));
}
{
  const log: string[] = [];
  console.log("some closes", P.some.call(plain(5, log), (x: number) => x === 1), log.join(","));
  const log2: string[] = [];
  console.log("find closes", P.find.call(plain(5, log2), (x: number) => x === 2), log2.join(","));
}

// --- re-entering a running helper is a TypeError ---
{
  let h: any;
  h = gen().map(() => h.next());
  console.log("running", errName(() => h.next()));
}

// --- next is read once (GetIteratorDirect) ---
{
  let reads = 0;
  let i = 0;
  const it: any = {
    get next() {
      reads++;
      return () => (i < 3 ? { value: i++, done: false } : { value: undefined, done: true });
    },
  };
  console.log("next once", JSON.stringify(P.map.call(it, (x: number) => x).toArray()), reads);
}

// --- overrides are observed ---
class Sub extends Iterator<number> {
  i = 0;
  next() {
    return this.i < 2 ? { value: this.i++, done: false } : { value: undefined, done: true };
  }
  map(fn: any): any {
    return "sub-map";
  }
}
console.log("sub override", new Sub().map((x: number) => x));
console.log("sub inherited", JSON.stringify(new Sub().filter(() => true).toArray()));
const own: any = [1, 2].values();
own.map = () => "own-map";
console.log("own override", own.map((x: number) => x));
const savedMap = P.map;
P.map = function (this: any, fn: any) {
  return "proto-map";
};
console.log("proto override gen", gen().map((x: number) => x));
console.log("proto override array iter", [1].values().map((x: number) => x));
console.log("proto override helper", gen().filter(() => true).map((x: number) => x));
P.map = savedMap;
console.log("restored", JSON.stringify(gen().map((x: number) => x * 10).toArray()));

// --- existing behaviour ---
console.log("chain", JSON.stringify(gen().map((x: number) => x * 2).filter((x: number) => x > 2).take(5).toArray()));
console.log("map iter", JSON.stringify(new Map([["a", 1], ["b", 2]]).keys().map((k: string) => k + k).toArray()));
console.log("set iter", new Set([1, 2, 3]).values().some((x: number) => x === 3));
console.log("string iter", JSON.stringify("hey"[Symbol.iterator]().drop(1).toArray()));
