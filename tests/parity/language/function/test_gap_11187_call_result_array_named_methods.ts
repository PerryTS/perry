// #11187: an Array-named method called on a receiver that is NOT proven to be
// an Array — a call result from an `any`-typed object, a builder method that
// returns `this`, an `await`ed value, a conditional — must run the receiver's
// own method. mongodb's `collection.find({}).sort({ a: 1 })` is the shape that
// failed: the chained call result was folded to `Array.prototype.sort` and
// threw "The comparison function must be either a function or undefined".
// Proven arrays (literals, Array-typed returns, split/Object.keys/Array.from
// chains) are controls: they must keep working exactly as before.

class Cursor {
  log: string[] = [];
  note(name: string, args: any[]): this {
    this.log.push(name + "(" + JSON.stringify(args) + ")");
    return this;
  }
  sort(sort: any, direction?: any) { return this.note("sort", [sort, direction]); }
  map(f: any) { return this.note("map", [typeof f]); }
  filter(f: any) { return this.note("filter", [typeof f]); }
  forEach(f: any) { return this.note("forEach", [typeof f]); }
  find(q: any) { return this.note("find", [q]); }
  findIndex(q: any) { return this.note("findIndex", [q]); }
  findLast(q: any) { return this.note("findLast", [q]); }
  findLastIndex(q: any) { return this.note("findLastIndex", [q]); }
  some(q: any) { return this.note("some", [q]); }
  every(q: any) { return this.note("every", [q]); }
  reduce(q: any, init?: any) { return this.note("reduce", [q, init]); }
  reduceRight(q: any, init?: any) { return this.note("reduceRight", [q, init]); }
  flat(depth?: any) { return this.note("flat", [depth]); }
  flatMap(f: any) { return this.note("flatMap", [typeof f]); }
  toSpliced(a?: any, b?: any) { return this.note("toSpliced", [a, b]); }
  toSorted(a?: any) { return this.note("toSorted", [a]); }
  toReversed() { return this.note("toReversed", []); }
  with(a: any, b: any) { return this.note("with", [a, b]); }
  join(sep?: any) { return this.note("join", [sep]); }
  slice(a?: any, b?: any) { return this.note("slice", [a, b]); }
  indexOf(v: any) { return this.note("indexOf", [v]); }
  includes(v: any) { return this.note("includes", [v]); }
  push(v: any) { return this.note("push", [v]); }
  pop() { return this.note("pop", []); }
  shift() { return this.note("shift", []); }
  unshift(v: any) { return this.note("unshift", [v]); }
  splice(a: any, b?: any) { return this.note("splice", [a, b]); }
  reverse() { return this.note("reverse", []); }
  fill(v: any) { return this.note("fill", [v]); }
  copyWithin(a: any, b: any) { return this.note("copyWithin", [a, b]); }
  concat(v: any) { return this.note("concat", [v]); }
  at(i: any) { return this.note("at", [i]); }
  entries() { return this.note("entries", []); }
  keys() { return this.note("keys", []); }
  values() { return this.note("values", []); }
  limit(n: number): this { return this.note("limit", [n]); }
}

function show(label: string, fn: () => any) {
  try {
    const r = fn();
    if (r instanceof Cursor) console.log(label, r.log.join(" "));
    else console.log(label, JSON.stringify(r));
  } catch (e: any) {
    console.log(label, "THREW", e && e.message);
  }
}

// 1. The mongodb shape: a method on an `any`-typed holder returns a cursor.
const holder: any = { find() { return new Cursor(); } };
show("find().sort", () => holder.find().sort({ a: 1 }));
show("find().sort(dir)", () => holder.find().sort("a", -1));
show("find().sort.limit", () => holder.find().sort({ a: 1 }).limit(2));
show("find().map", () => holder.find().map((d: any) => d));
show("find().filter", () => holder.find().filter((d: any) => d));
show("find().forEach", () => holder.find().forEach((d: any) => d));
show("find().find", () => holder.find().find({ q: 1 }));
show("find().findIndex", () => holder.find().findIndex({ q: 1 }));
show("find().findLast", () => holder.find().findLast({ q: 1 }));
show("find().findLastIndex", () => holder.find().findLastIndex({ q: 1 }));
show("find().some", () => holder.find().some({ q: 1 }));
show("find().every", () => holder.find().every({ q: 1 }));
show("find().reduce", () => holder.find().reduce({ q: 1 }, 0));
show("find().reduceRight", () => holder.find().reduceRight({ q: 1 }, 0));
show("find().flat", () => holder.find().flat());
show("find().flat(2)", () => holder.find().flat(2));
show("find().flatMap", () => holder.find().flatMap((d: any) => d));
show("find().toSpliced", () => holder.find().toSpliced(1, 2));
show("find().toSpliced()", () => holder.find().toSpliced());
show("find().toSorted", () => holder.find().toSorted({ a: 1 }));
show("find().toReversed", () => holder.find().toReversed());
show("find().with", () => holder.find().with(0, 1));
show("find().join", () => holder.find().join(","));
show("find().slice", () => holder.find().slice(1, 2));
show("find().indexOf", () => holder.find().indexOf(3));
show("find().includes", () => holder.find().includes(3));
show("find().push", () => holder.find().push(3));
show("find().pop", () => holder.find().pop());
show("find().shift", () => holder.find().shift());
show("find().unshift", () => holder.find().unshift(3));
show("find().splice", () => holder.find().splice(1, 1));
show("find().reverse", () => holder.find().reverse());
show("find().fill", () => holder.find().fill(0));
show("find().copyWithin", () => holder.find().copyWithin(0, 1));
show("find().concat", () => holder.find().concat([1]));
show("find().at", () => holder.find().at(0));
show("find().entries", () => holder.find().entries());
show("find().keys", () => holder.find().keys());
show("find().values", () => holder.find().values());

// 2. A builder chain that returns `this` — the receiver of `.sort` is the
// result of a typed class method, not an array.
const cur = new Cursor();
show("limit().sort", () => cur.limit(1).sort({ b: -1 }));
show("limit().flat", () => cur.limit(1).flat());
show("limit().toSpliced", () => cur.limit(1).toSpliced(0, 1));
show("limit().map", () => cur.limit(1).map((x: any) => x));

// 3. A free function returning `any`.
function openCursor(): any { return new Cursor(); }
show("fn().sort", () => openCursor().sort({ c: 1 }));
show("fn().flat", () => openCursor().flat());
show("fn().reduce", () => openCursor().reduce({ q: 1 }));

// 4. Conditional / parenthesised / member receivers.
const flag = Math.random() >= 0;
show("cond.sort", () => (flag ? new Cursor() : holder.find()).sort({ d: 1 }));
show("cond.map", () => (flag ? holder.find() : new Cursor()).map((x: any) => x));
const wrap: any = { cur: new Cursor() };
show("member.sort", () => wrap.cur.sort({ e: 1 }));
show("member.flat", () => wrap.cur.flat());
show("member.toSpliced", () => wrap.cur.toSpliced(0));

// 5. Controls: a call result from `any` that IS an array at runtime still
// behaves as an Array (dynamic dispatch reaches the Array method).
const src: any = { list() { return [3, 1, 2]; }, nested() { return [[1], [2, [3]]]; } };
show("anyArr.sort", () => src.list().sort((a: number, b: number) => a - b));
show("anyArr.map", () => src.list().map((x: number) => x * 2));
show("anyArr.flat", () => src.nested().flat());
show("anyArr.toSpliced", () => src.list().toSpliced(0, 1));
show("anyArr.reduce", () => src.list().reduce((a: number, b: number) => a + b, 0));
show("anyArr.find", () => src.list().find((x: number) => x < 3));

// 6. Controls: proven arrays (literal, typed return, builtin producers).
function nums(): number[] { return [5, 3, 9, 1]; }
show("lit.sort", () => [3, 1, 2].sort((a, b) => a - b));
show("typed().sort", () => nums().sort((a, b) => a - b));
show("typed().flat", () => nums().map((n) => [n, n]).flat());
show("typed().toSpliced", () => nums().toSpliced(1, 2));
show("typed().reduce", () => nums().reduce((a, b) => a + b, 0));
show("split.sort", () => "b,c,a".split(",").sort((x, y) => (x < y ? -1 : 1)));
show("keys.sort", () => Object.keys({ z: 1, y: 2 }).sort((x, y) => (x < y ? -1 : 1)));
show("from.flat", () => Array.from([[1, 2], [3]]).flat());
show("slice.sort", () => nums().slice().sort((a, b) => b - a));
show("filter.find", () => nums().filter((n) => n > 2).find((n) => n > 4));
show("map.toSpliced", () => [1, 2, 3].map((n) => n * 10).toSpliced(0, 1));

// 7. Async: the awaited result of an `any` promise.
async function main() {
  const coll: any = { find: async () => new Cursor(), rows: async () => [2, 1] };
  const c = await coll.find();
  show("awaited-local.sort", () => c.sort({ f: 1 }));
  const viaAwait = (await coll.find()).sort({ g: 1 });
  console.log("await().sort", viaAwait.log.join(" "));
  const rowsSorted = (await coll.rows()).sort((a: number, b: number) => a - b);
  console.log("await().rows.sort", JSON.stringify(rowsSorted));
}
main();
