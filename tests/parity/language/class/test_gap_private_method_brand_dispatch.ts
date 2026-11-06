// Private methods and fields on hot paths, the brand check failing with a
// TypeError for receivers that never ran the declaring constructor, and public
// methods of classes whose instances carry private state.

class Counter {
  #n = 0;
  #step = 1;
  #bump(x: number): void { this.#n = (this.#n + x * this.#step) | 0; }
  add(i: number): number { this.#bump(i); return this.#n; }
  static peek(o: any): number { return o.#n; }
  static has(o: any): boolean { return #n in o; }
  callBump(o: any): void { this.#bump.call(o, 1); }
}

class Lru {
  #map = new Map<string, number>();
  #hits = 0;
  #touch(k: string): number | undefined {
    const v = this.#map.get(k);
    if (v !== undefined) { this.#hits++; this.#map.delete(k); this.#map.set(k, v); }
    return v;
  }
  get(k: string): number | undefined { return this.#touch(k); }
  set(k: string, v: number): this { this.#map.set(k, v); if (this.#map.size > 8) this.#map.delete(this.#map.keys().next().value!); return this; }
  get hits(): number { return this.#hits; }
}

const c = new Counter();
let last = 0;
for (let i = 0; i < 100000; i++) last = c.add(i);
console.log("counter", last, Counter.peek(c), Counter.has(c), Counter.has({}));

const lru = new Lru();
let found = 0;
for (let i = 0; i < 20000; i++) {
  lru.set("k" + (i % 12), i);
  if (lru.get("k" + ((i * 7) % 12)) !== undefined) found++;
}
console.log("lru", found, lru.hits);

function attempt(label: string, f: () => unknown): void {
  try { f(); console.log(label, "no error"); }
  catch (e) { console.log(label, e instanceof TypeError); }
}
attempt("peek foreign", () => Counter.peek({}));
attempt("add on foreign", () => Counter.prototype.add.call({}, 1));
attempt("bump foreign", () => c.callBump({}));
attempt("peek object with same keys", () => Counter.peek(Object.create(Counter.prototype)));

class Sub extends Counter { extra = 1; }
const sub = new Sub();
for (let i = 0; i < 10; i++) sub.add(i);
console.log("subclass", Counter.peek(sub), Counter.has(sub), sub.extra);
