// #11128: a user method whose name collides with a builtin fast-path name
// (push, pop, shift, get, set, has, add, delete, slice, ...) must run the
// user method when the receiver is built by `new` from a class VALUE that is
// not statically a class (an `any` binding, a factory result, a destructured
// "require"-shaped namespace). Shape from redis@6.1.0 `linked-list.js`:
// `const { SinglyLinkedList } = require(...)`, `s.push(1); s.length`.

class R {
  n = 0;
  log: string[] = [];
  push(v: number) { this.n += v; this.log.push("push"); return this; }
  pop() { this.log.push("pop"); return "p" + this.n; }
  shift() { this.log.push("shift"); return "s" + this.n; }
  unshift(v: number) { this.n -= v; this.log.push("unshift"); return this.n; }
  get(k: string) { this.log.push("get"); return "g:" + k; }
  set(k: string, v: number) { this.log.push("set"); this.n = v; return this; }
  has(k: string) { this.log.push("has"); return k === "yes"; }
  add(v: number) { this.n += v * 10; this.log.push("add"); return this; }
  delete(k: string) { this.log.push("delete"); return k.length; }
  slice(a: number) { this.log.push("slice"); return "sl" + a; }
  indexOf(x: number) { this.log.push("indexOf"); return 42 + x; }
  includes(x: number) { this.log.push("includes"); return x === 7; }
  map(f: (x: number) => number) { this.log.push("map"); return f(this.n); }
  forEach(f: (x: number) => void) { this.log.push("forEach"); f(this.n); }
  join(sep: string) { this.log.push("join"); return ["a", "b"].join(sep); }
  sort() { this.log.push("sort"); return "sorted"; }
  reverse() { this.log.push("reverse"); return "reversed"; }
  splice(a: number) { this.log.push("splice"); return a * 2; }
  concat(s: string) { this.log.push("concat"); return "c" + s; }
  at(i: number) { this.log.push("at"); return "at" + i; }
  fill(v: number) { this.n = v; this.log.push("fill"); return this; }
  clear() { this.log.push("clear"); this.n = 0; }
}

function exercise(label: string, r: any) {
  const ret = r.push(1);
  console.log(label, "push", r.n, ret === r);
  console.log(label, "pop", r.pop(), "shift", r.shift(), "unshift", r.unshift(1));
  console.log(label, "get", r.get("k"), "has", r.has("yes"), r.has("no"));
  const s = r.set("k", 5);
  console.log(label, "set", r.n, s === r);
  const a = r.add(2);
  console.log(label, "add", r.n, a === r);
  console.log(label, "delete", r.delete("abc"), "slice", r.slice(3));
  console.log(label, "indexOf", r.indexOf(1), "includes", r.includes(7), r.includes(8));
  console.log(label, "map", r.map((x: number) => x + 1));
  let seen = -1;
  r.forEach((x: number) => { seen = x; });
  console.log(label, "forEach", seen, "join", r.join("-"));
  console.log(label, "sort", r.sort(), "reverse", r.reverse(), "splice", r.splice(4));
  console.log(label, "concat", r.concat("z"), "at", r.at(2));
  const f = r.fill(9);
  console.log(label, "fill", r.n, f === r);
  r.clear();
  console.log(label, "clear", r.n);
  console.log(label, "log", r.log.join(","));
}

// 1. `any`-typed class value in a local, called directly at the call site
//    (NOT through a helper, so the receiver's static type comes from `new`).
{
  const C: any = R;
  const r = new C();
  const ret = r.push(1);
  console.log("any-local push", r.n, ret === r);
  r.push(2);
  console.log("any-local push2", r.n, r.pop(), r.shift());
  console.log("any-local get/has", r.get("x"), r.has("yes"));
  const s = r.set("x", 3);
  console.log("any-local set", r.n, s === r);
  r.add(1);
  console.log("any-local add", r.n, r.delete("ab"), r.slice(2));
  console.log("any-local indexOf", r.indexOf(0), r.includes(7));
  console.log("any-local sort", r.sort(), r.reverse(), r.splice(1), r.concat("q"), r.at(0));
  console.log("any-local log", r.log.join(","));
  exercise("any-local", new C());
}

// 2. class value returned by a factory
function makeClass(): any {
  return class Stack {
    items: number[] = [];
    push(v: number) { this.items.push(v); return this.items.length * 100; }
    pop() { return this.items.pop(); }
    get length() { return this.items.length; }
  };
}
{
  const K = makeClass();
  const st = new K();
  console.log("factory push", st.push(5), st.push(6), st.length);
  console.log("factory pop", st.pop(), st.length);
}

// 3. require-shaped namespace destructuring (redis linked-list.js shape)
class SinglyLinkedList {
  #head: any = undefined;
  #tail: any = undefined;
  #length = 0;
  get length() { return this.#length; }
  push(value: any) {
    ++this.#length;
    const node = { value, next: undefined as any };
    if (this.#head === undefined) return this.#head = this.#tail = node;
    return this.#tail = this.#tail.next = node;
  }
  shift() {
    if (this.#head === undefined) return undefined;
    const node = this.#head;
    if (--this.#length === 0) { this.#head = this.#tail = undefined; }
    else { this.#head = node.next; }
    return node.value;
  }
}
class EmptyAwareSinglyLinkedList extends SinglyLinkedList {
  events: string[] = [];
  push(value: any) {
    if (this.length === 0) this.events.push("nonEmpty");
    return super.push(value);
  }
}
const lib: any = { SinglyLinkedList, EmptyAwareSinglyLinkedList };
{
  const { SinglyLinkedList: SLL, EmptyAwareSinglyLinkedList: EASLL } = lib;
  const s = new SLL();
  s.push(1);
  console.log("require-shape length", s.length);
  console.log("require-shape shift", s.shift(), s.length);
  const e = new EASLL();
  e.push("a");
  e.push("b");
  console.log("require-shape empty-aware", e.length, e.events.join(","), e.shift());
  const viaMember = new lib.SinglyLinkedList();
  viaMember.push(3);
  viaMember.push(4);
  console.log("member-ctor", viaMember.length, viaMember.shift());
}

// 4. typed receivers (controls): statically proven class instances
{
  const C: any = R;
  const typed: R = new C();
  typed.push(4);
  console.log("typed-annot", typed.n, typed.get("t"), typed.has("yes"));
  const direct = new R();
  const d = direct.push(2);
  console.log("typed-direct", direct.n, d === direct, direct.pop());
}

// 5. a real Array reached through an `any` constructor value must keep
//    Array semantics under the same method names.
{
  const A: any = Array;
  const arr = new A();
  console.log("array-any push", arr.push(1), arr.push(2, 3), arr.length);
  console.log("array-any pop/shift", arr.pop(), arr.shift(), arr.length, arr.slice(0).join("|"));
  arr.unshift(0);
  console.log("array-any", arr.indexOf(2), arr.includes(0), arr.join(","), arr.at(-1));
  const M: any = Map;
  const m = new M();
  m.set("a", 1);
  console.log("map-any", m.get("a"), m.has("a"), m.delete("a"), m.size);
  const S: any = Set;
  const st = new S();
  st.add(1); st.add(1); st.add(2);
  console.log("set-any", st.size, st.has(2), st.delete(2), st.size);
  const plain: number[] = [];
  plain.push(7, 8);
  console.log("array-typed", plain.length, plain.pop(), plain.length);
}

// 6. the instance reaches the call through other bindings
interface Pushy { n: number; push(v: number): Pushy; get(k: string): string; }
{
  const C: any = R;
  class Holder {
    q = new C();
    run() {
      this.q.push(1);
      this.q.add(1);
      return this.q.n + ":" + this.q.get("a") + ":" + this.q.delete("xy");
    }
  }
  console.log("field", new Holder().run());
  let r2 = new C();
  r2 = new C();
  r2.push(2);
  console.log("let-reassigned", r2.n, r2.has("yes"));
  function useIt(x: InstanceType<typeof R>) {
    x.push(3);
    x.add(1);
    return x.n + ":" + x.get("i");
  }
  console.log("instancetype-param", useIt(new C()));
  const iq: Pushy = new C();
  iq.push(4);
  console.log("interface-typed", iq.n, iq.get("b"));
  function mk() { return new C(); }
  const m = mk();
  m.push(6);
  m.add(1);
  console.log("fn-return", m.n, m.get("z"), m.has("yes"), m.delete("q"));
  const holder = { k: new C() };
  holder.k.push(8);
  console.log("object-prop", holder.k.n, holder.k.get("o"));
  const G = class<T> { items: T[] = []; push(v: T) { this.items.push(v); return -this.items.length; } };
  const GA: any = G;
  const gi = new GA();
  console.log("generic-class-value", gi.push(1), gi.push(2), gi.items.length);
  const gt = new GA<number>();
  console.log("generic-type-args", gt.push(5), gt.items.length);
}
