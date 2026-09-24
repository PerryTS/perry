// #11170: a generator method keyed by a well-known symbol must keep its
// method receiver. `*[Symbol.iterator]()` used to be lifted into a top-level
// function whose `this` rewrite missed `#private` receivers, arrows and
// template literals, so `this.#head` threw "Cannot access private member from
// an object whose class did not declare it" (@redis/client's linked lists).
// An instance `[Symbol.hasInstance]()` method (generator or not) read as
// `undefined`.

function tryIt(label: string, f: () => any): void {
  try {
    console.log(label, f());
  } catch (e: any) {
    // Only the error class: the private-access message text differs from V8's.
    console.log(label, "threw", e.constructor.name);
  }
}

const K = "k";
class G {
  #items = [1, 2, 3];
  pub = 5;
  #m() {
    return "m" + this.pub;
  }
  *plain() {
    yield this.#items.length;
  }
  *[Symbol.iterator]() {
    yield this.#items.length;
  }
  *[K]() {
    yield this.#items.length;
  }
  *["lit"]() {
    yield this.#items.length;
  }
  [Symbol.asyncIterator]() {
    return this.#items.length;
  }
  *[Symbol.hasInstance]() {
    yield this === undefined ? "undef" : typeof this;
  }
}
const g: any = new G();
tryIt("plain", () => g.plain().next().value);
tryIt("sym direct", () => g[Symbol.iterator]().next().value);
tryIt("sym spread", () => [...g].join(","));
tryIt("const key", () => g[K]().next().value);
tryIt("literal key", () => g["lit"]().next().value);
tryIt("sym non-gen", () => g[Symbol.asyncIterator]());
tryIt("sym this", () => g[Symbol.hasInstance]().next().value);
tryIt("hasInstance typeof", () => typeof g[Symbol.hasInstance]);

// The @redis/client shape: a linked list iterated with for-of from another
// class's private field.
class LinkedList<T> {
  #head: { value: T; next: any } | undefined = undefined;
  #tail: { value: T; next: any } | undefined = undefined;
  #length = 0;
  get length() {
    return this.#length;
  }
  push(value: T) {
    const node = { value, next: undefined };
    if (this.#tail) this.#tail.next = node;
    else this.#head = node;
    this.#tail = node;
    this.#length++;
    return node;
  }
  *[Symbol.iterator]() {
    let node = this.#head;
    while (node !== undefined) {
      yield node;
      node = node.next;
    }
  }
}
class Queue {
  #waiting = new LinkedList();
  add(s: string) {
    this.#waiting.push(s);
  }
  names() {
    const out: string[] = [];
    for (const node of this.#waiting) out.push(node.value);
    return out.join("|");
  }
}
const q = new Queue();
q.add("SET");
q.add("GET");
tryIt("queue for-of", () => q.names());
const ll = new LinkedList();
ll.push(10);
ll.push(20);
tryIt("list spread", () => [...ll].map((n) => n.value).join(","));
tryIt("list Array.from", () => Array.from(ll, (n) => n.value * 2).join(","));
tryIt("list destructure", () => {
  const [a, b] = ll;
  return a.value + b.value;
});
tryIt("list for-of new", () => {
  let s = 0;
  const l2 = new LinkedList();
  l2.push(3);
  l2.push(4);
  for (const n of l2) s += n.value;
  return s;
});

// Other receiver shapes inside the symbol-keyed generator body.
class Shapes {
  #items = [1, 2, 3];
  pub = 5;
  #m() {
    return "m" + this.pub;
  }
  *[Symbol.iterator]() {
    const f = () => this.pub;
    yield "arrow " + f();
    yield `tpl ${this.pub}`;
    let i = 0;
    do {
      i++;
    } while (i < this.pub);
    yield "do " + i;
    outer: for (const x of [1]) {
      yield "lab " + (x + this.pub);
      break outer;
    }
    yield "pm " + this.#m();
    const g2 = () => this.#items.length;
    yield "arrowpriv " + g2();
    yield "brand " + (#items in this);
    for (const v of this.#items) yield "inner " + v;
    yield* this.#items;
  }
}
tryIt("shapes", () => [...new Shapes()].join(";"));

// A foreign receiver must still be rejected.
class Other {
  #x = 1;
  *[Symbol.iterator]() {
    yield this.#x;
  }
}
tryIt("foreign receiver", () => {
  const it = Other.prototype[Symbol.iterator].call({});
  return it.next().value;
});

// Async generator keyed by Symbol.asyncIterator.
class AsyncList {
  #items = ["a", "b"];
  async *[Symbol.asyncIterator]() {
    for (const v of this.#items) yield v;
  }
}

// Static variants.
class StaticGen {
  static #count = 2;
  static *[Symbol.iterator]() {
    yield StaticGen.#count;
    yield this.#count;
  }
  static *[K]() {
    yield this.#count;
  }
}
tryIt("static sym gen", () => [...(StaticGen as any)[Symbol.iterator]()].join(","));
tryIt("static const-key gen", () => (StaticGen as any)[K]().next().value);

// Per-evaluation classes: a class expression evaluated inside a function.
function makeList() {
  return class {
    #items: number[];
    constructor(...items: number[]) {
      this.#items = items;
    }
    *[Symbol.iterator]() {
      yield* this.#items;
    }
  };
}
const L1 = makeList();
const L2 = makeList();
tryIt("per-eval spread", () => [...new L1(1, 2)].concat([...new L2(3)]).join(","));
tryIt("per-eval cross brand", () => {
  const it = (L1.prototype as any)[Symbol.iterator].call(new L2(9));
  return it.next().value;
});

// Well-known-symbol-keyed non-generator instance methods.
class WK {
  #tag = "wk";
  [Symbol.hasInstance]() {
    return "hi " + this.#tag;
  }
  [Symbol.split]() {
    return "split " + this.#tag;
  }
}
const wk: any = new WK();
tryIt("wk hasInstance", () => wk[Symbol.hasInstance]());
tryIt("wk split", () => wk[Symbol.split]());

async function main() {
  const out: string[] = [];
  for await (const v of new AsyncList()) out.push(v);
  console.log("async gen", out.join(","));
}
main();
