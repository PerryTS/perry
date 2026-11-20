// #11184: computed static generators retain class receivers in resume closures.
const K = "k";
class S {
  static #c = 2;
  static pub = 7;
  static *a() { yield this.#c; }
  static *[K]() { yield this.pub; yield S.#c; yield this.#c; }
  static *[K + "finally"]() { try { yield 1; } finally { yield this.pub; } }
  static [K + "2"]() { return this.#c; }
}
console.log("a", [...S.a()].join(","));
console.log("K", [...(S as any)[K]()].join(","));
console.log("K2", (S as any)["k2"]());
const closing = (S as any)[K + "finally"]();
console.log("finally start", closing.next().value);
console.log("finally return", closing.return().value);

class Instance {
  #x = 9;
  *[K]() { yield this.#x; }
}
console.log("instance", [...(new Instance() as any)[K]()].join(","));

class Iter {
  static value = 11;
  static *[Symbol.iterator]() { yield this.value; }
}
console.log("symbol", [...Iter].join(","));

class AsyncStatic {
  static #x = 13;
  static value = 17;
  static async *[K]() { yield this.value; yield this.#x; }
}
class AsyncInstance {
  #x = 19;
  async *[K]() { yield this.#x; }
}
async function run() {
  const stat = (AsyncStatic as any)[K]();
  console.log("async static", (await stat.next()).value, (await stat.next()).value);
  console.log("async static done", (await stat.next()).done);
  const inst = (new AsyncInstance() as any)[K]();
  console.log("async instance", (await inst.next()).value);
  console.log("async instance done", (await inst.next()).done);
}
run();
