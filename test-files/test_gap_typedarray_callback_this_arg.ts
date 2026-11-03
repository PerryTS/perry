// %TypedArray%.prototype callback methods bind `thisArg` as the callback's
// `this` (Uint8Array and the other element kinds take different runtime
// paths), and `new TypedArray(iterable)` calls the source's @@iterator method
// with the source as `this`.
class Svc {
  marker = 3;
  seen = 0;
  note(v: number): number {
    this.seen += v;
    return this.seen;
  }
}

function run(label: string, make: () => any): void {
  const ta = make();
  const o = new Svc();
  const out: string[] = [];
  ta.forEach(function (this: Svc, v: number) {
    this.note(v);
  }, o);
  out.push("forEach " + o.seen);

  const m = new Svc();
  const mapped = ta.map(function (this: Svc, v: number) {
    return this.note(v);
  }, m);
  out.push("map " + Array.from(mapped).join(","));

  const f = new Svc();
  const filtered = ta.filter(function (this: Svc, v: number) {
    this.note(v);
    return this.marker === 3;
  }, f);
  out.push("filter " + filtered.length + " " + f.seen);

  const e = new Svc();
  out.push("every " + ta.every(function (this: Svc) { return this.marker === 3; }, e));
  out.push("some " + ta.some(function (this: Svc) { return this.marker === 4; }, e));
  out.push("find " + ta.find(function (this: Svc, v: number) { return v === this.marker; }, e));
  out.push("findIndex " + ta.findIndex(function (this: Svc, v: number) { return v === this.marker; }, e));
  out.push("findLast " + ta.findLast(function (this: Svc, v: number) { return v < this.marker; }, e));
  out.push("findLastIndex " + ta.findLastIndex(function (this: Svc, v: number) { return v < this.marker; }, e));

  const plain: number[] = [];
  ta.forEach(function (this: any) {
    plain.push(typeof this);
  });
  out.push("nothis " + plain.join(","));
  console.log(label + ": " + out.join(" | "));
}

run("Uint8Array", () => new Uint8Array([1, 2, 3, 4]));
run("Float64Array", () => new Float64Array([1, 2, 3, 4]));
run("Int32Array", () => new Int32Array([1, 2, 3, 4]));

// A thisArg object that must stay reachable across callback-triggered GCs.
const holder = { n: 0 };
new Uint8Array([1, 2, 3]).forEach(function (this: { n: number }) {
  const junk: number[][] = [];
  for (let i = 0; i < 2000; i++) junk.push([i]);
  this.n += junk.length;
}, holder);
console.log("held " + holder.n);

// Receiver of the iterator method of an iterable passed to the constructor.
const src: any = new Svc();
src[Symbol.iterator] = function* (this: Svc) {
  yield this.marker;
  yield this.marker + 1;
};
console.log("ctor " + Array.from(new Uint8Array(src)).join(","));
console.log("ctor f64 " + Array.from(new Float64Array(src)).join(","));
