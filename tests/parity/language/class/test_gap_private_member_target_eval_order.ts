// #11150: in `this.#tail.next = this.#tail = node` the member target's OBJECT
// (`this.#tail`) must be evaluated BEFORE the right-hand side, which reassigns
// `#tail`. Perry evaluated the RHS first, so the store landed on the NEW node
// (a self-loop) and a linked list lost every entry after its head. This covers
// the private-field-object forms across every assignment operator.

const log: string[] = [];
function t(tag: string, v: any): any {
  log.push(tag);
  return v;
}
function flush(label: string) {
  console.log(label + ": " + log.join(","));
  log.length = 0;
}

// --- 1. The redis @redis/client linked-list idiom -------------------------
class SinglyLinkedList {
  #length = 0;
  #head: any = undefined;
  #tail: any = undefined;
  push(value: any) {
    ++this.#length;
    const node = { value, next: undefined as any };
    if (this.#head === undefined) {
      return (this.#head = this.#tail = node);
    }
    return (this.#tail.next = this.#tail = node);
  }
  shift() {
    if (this.#head === undefined) return undefined;
    const node = this.#head;
    if (--this.#length === 0) {
      this.#head = this.#tail = undefined;
    } else {
      this.#head = node.next;
    }
    return node.value;
  }
  get length() {
    return this.#length;
  }
}
{
  const l = new SinglyLinkedList();
  for (let i = 1; i <= 4; i++) l.push(i);
  const out: any[] = [];
  for (let i = 0; i < 4; i++) out.push(l.shift());
  console.log("singly:", out.join(","), l.length);
}

class DoublyLinkedList {
  #length = 0;
  #head: any = undefined;
  #tail: any = undefined;
  push(value: any) {
    ++this.#length;
    if (this.#tail === undefined) {
      return (this.#head = this.#tail = { previous: this.#head, next: undefined, value });
    }
    return (this.#tail = this.#tail.next = { previous: this.#tail, next: undefined, value });
  }
  unshift(value: any) {
    ++this.#length;
    if (this.#head === undefined) {
      return (this.#head = this.#tail = { previous: undefined, next: undefined, value });
    }
    return (this.#head = this.#head.previous = { previous: undefined, next: this.#head, value });
  }
  forwards() {
    const out: any[] = [];
    let node = this.#head;
    while (node !== undefined) {
      out.push(node.value);
      node = node.next;
    }
    return out.join(",");
  }
  backwards() {
    const out: any[] = [];
    let node = this.#tail;
    while (node !== undefined) {
      out.push(node.value);
      node = node.previous;
    }
    return out.join(",");
  }
}
{
  const d = new DoublyLinkedList();
  d.push(2);
  d.push(3);
  d.unshift(1);
  d.push(4);
  d.unshift(0);
  console.log("doubly:", d.forwards(), "|", d.backwards());
}

// --- 2. Every operator form, object = private field -----------------------
class Box {
  #o: any;
  constructor(o: any) {
    this.#o = o;
  }
  get o() {
    return this.#o;
  }
  assign(other: any) {
    return (this.#o.v = (this.#o = other, "assigned"));
  }
  computed(other: any, k: string) {
    return (this.#o[t("key", k)] = t("rhs", (this.#o = other, "computed")));
  }
  deep(other: any) {
    return (this.#o.inner.v = (this.#o = other, "deep"));
  }
  add(other: any) {
    return (this.#o.n += (this.#o = other, 10));
  }
  mul(other: any) {
    return (this.#o.n *= (this.#o = other, 3));
  }
  nullish(other: any) {
    return (this.#o.q ??= (this.#o = other, "nq"));
  }
  or(other: any) {
    return (this.#o.q ||= (this.#o = other, "oq"));
  }
  and(other: any) {
    return (this.#o.q &&= (this.#o = other, "aq"));
  }
  computedAdd(other: any, k: string) {
    return (this.#o[k] += (this.#o = other, 5));
  }
  swapNext(other: any) {
    // right-nested chain the other way round
    return (this.#o = this.#o.next = other);
  }
}
function fresh(id: string) {
  return { id, n: 1, q: undefined as any, inner: { id: id + ".inner" } as any } as any;
}
{
  const ops: [string, (b: Box, other: any) => any][] = [
    ["=", (b, o) => b.assign(o)],
    ["deep =", (b, o) => b.deep(o)],
    ["+=", (b, o) => b.add(o)],
    ["*=", (b, o) => b.mul(o)],
    ["??=", (b, o) => b.nullish(o)],
    ["||=", (b, o) => b.or(o)],
    ["&&=", (b, o) => b.and(o)],
    ["[k]+=", (b, o) => b.computedAdd(o, "n")],
    ["= =", (b, o) => b.swapNext(o)],
  ];
  for (const [name, fn] of ops) {
    const a = fresh("a");
    const b = fresh("b");
    if (name === "&&=") a.q = "truthy";
    const box = new Box(a);
    const r = fn(box, b);
    console.log(
      name.padEnd(6),
      "ret=" + JSON.stringify(r),
      "a=" + JSON.stringify({ n: a.n, q: a.q, v: a.v, iv: a.inner.v, nx: a.next && a.next.id }),
      "b=" + JSON.stringify({ n: b.n, q: b.q, v: b.v, iv: b.inner.v, nx: b.next && b.next.id }),
      "now=" + box.o.id,
    );
  }
  const a = fresh("a");
  const b = fresh("b");
  const box = new Box(a);
  box.computed(b, "c");
  console.log("[k] =", a.c, b.c, box.o.id);
  flush("computed order");
}

// --- 3. ++ / -- on a private-field object (object read exactly once) ------
class Counter {
  #o: any = { n: 5 };
  #reads = 0;
  get #g() {
    this.#reads++;
    return this.#o;
  }
  bump() {
    this.#o.n++;
    ++this.#o.n;
    this.#o.n--;
    const post = this.#g.n++;
    const pre = --this.#g.n;
    return [this.#o.n, post, pre, this.#reads].join(",");
  }
}
console.log("update:", new Counter().bump());

// --- 4. Private getter as the object: getter runs before the RHS ---------
class Getter {
  #store: any = { id: "first" };
  get #g() {
    log.push("get#g");
    return this.#store;
  }
  set #g(v: any) {
    log.push("set#g");
    this.#store = v;
  }
  run() {
    const first = this.#store;
    const second = { id: "second" };
    this.#g.x = t("rhs", (this.#g = second, 1));
    this.#g.y += t("rhs+", (this.#g = first, 2));
    return [first.x, second.x, first.y, second.y].join(",");
  }
}
console.log("getter:", new Getter().run());
flush("getter order");

// --- 5. Another instance's private field as the object (obj.#a.x) --------
class Peer {
  #a: any;
  constructor(a: any) {
    this.#a = a;
  }
  static swap(p: Peer, other: any) {
    p.#a.x = (p.#a = other, "px");
    return p.#a;
  }
  static peek(p: Peer) {
    return p.#a;
  }
}
{
  const a: any = { id: "a" };
  const b: any = { id: "b" };
  const p = new Peer(a);
  Peer.swap(p, b);
  console.log("peer:", a.x, b.x, Peer.peek(p).id);
}

// --- 6. Nested private target: this.#a.#b where #a is reassigned ---------
class Node2 {
  #b: any = "init";
  #a: Node2 | undefined;
  label: string;
  constructor(label: string) {
    this.label = label;
  }
  setA(n: Node2) {
    this.#a = n;
  }
  nest(other: Node2) {
    this.#a!.#b = (this.#a = other, "nested:" + other.label);
    return this.#a!.label;
  }
  nestAdd(other: Node2) {
    this.#a!.#b += (this.#a = other, "+");
    return this.#a!.label;
  }
  static b(n: Node2) {
    return n.#b;
  }
}
{
  const root = new Node2("root");
  const x = new Node2("x");
  const y = new Node2("y");
  root.setA(x);
  const now = root.nest(y);
  console.log("nested:", Node2.b(x), Node2.b(y), now);
  const z = new Node2("z");
  const now2 = root.nestAdd(z);
  console.log("nested+=:", Node2.b(y), Node2.b(z), now2);
}

// --- 7. super.x.y where the RHS replaces the object super.x returns -------
class Base {
  static cur: any = { id: "base1" };
  get x() {
    log.push("super.x");
    return Base.cur;
  }
}
class Derived extends Base {
  run() {
    const first = Base.cur;
    const second = { id: "base2" } as any;
    super.x.y = t("rhs", (Base.cur = second, "sy"));
    return [first.y, second.y].join(",");
  }
}
console.log("super:", new Derived().run());
flush("super order");

// --- 8. Public-field control (was already correct) -------------------------
class Pub {
  tail: any;
  push(node: any) {
    if (this.tail === undefined) {
      this.tail = node;
      return;
    }
    this.tail.next = this.tail = node;
  }
}
{
  const p = new Pub();
  const a: any = { id: "a" };
  const b: any = { id: "b" };
  p.push(a);
  p.push(b);
  console.log("public:", a.next && a.next.id, b.next && b.next.id);
}

// --- 9. Other base shapes: evaluated exactly once, before the RHS ----------
{
  let built = 0;
  class Tracker {
    x: any;
    constructor() {
      built++;
      log.push("new");
    }
  }
  const made = (new Tracker().x = t("rhs", 1));
  console.log("new base:", made, built);
  flush("new order");

  let flag = true;
  const p: any = { id: "p" };
  const q: any = { id: "q" };
  (t("cond", flag) ? p : q).z = t("rhs", (flag = false, "z"));
  console.log("conditional base:", p.z, q.z);
  flush("conditional order");

  class Stmt {
    #o: any;
    constructor(o: any) {
      this.#o = o;
    }
    run(other: any) {
      this.#o.n += (this.#o = other, 100);
      this.#o.m = 0;
      this.#o.m ||= (this.#o = other, 7);
    }
  }
  const a: any = { n: 1 };
  const b: any = { n: 2 };
  new Stmt(a).run(b);
  console.log("statement compound:", a.n, b.n, a.m, b.m);
}
