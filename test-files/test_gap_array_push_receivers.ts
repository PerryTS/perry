// `push` on a plain dense Array takes a header-word fast receiver check before
// the complete resolver. Every receiver that is not a plain, descriptor-free
// Array while no prototype index exists must keep full spec behavior: frozen
// and sealed arrays, arrays with accessors, sparse arrays,
// subclasses, Proxies, typed arrays reached through `any`, and a program-wide
// indexed property on Array.prototype installed half way through.

function show(label: string, f: () => unknown): void {
  try {
    console.log(label, JSON.stringify(f()));
  } catch (e) {
    console.log(label, "threw", (e as Error).constructor.name);
  }
}

const nums: number[] = [];
for (let i = 0; i < 20; i++) nums.push(i * 0.5);
show("numbers", () => [nums.length, nums[19], nums.pop(), nums.length]);

const mixed: unknown[] = [];
for (let i = 0; i < 10; i++) mixed.push(i % 2 === 0 ? { i } : "s" + i);
show("mixed", () => mixed.map((v) => (typeof v === "string" ? v : (v as { i: number }).i)));

const frozen = Object.freeze([1, 2]) as number[];
show("frozen", () => frozen.push(3));
const sealed = Object.seal([1, 2]) as number[];
show("sealed", () => sealed.push(3));

const withLength: number[] = [1, 2, 3];
Object.defineProperty(withLength, "length", { writable: false });
show("readonly length", () => withLength.push(4));

const withSetter: number[] = [1, 2];
let setterHits = 0;
Object.defineProperty(withSetter, 2, {
  get() {
    return 99;
  },
  set(v: number) {
    setterHits += v;
  },
  configurable: true,
});
show("own index accessor", () => [withSetter.push(5), setterHits, withSetter.length]);

const sparse: number[] = [];
sparse[5] = 1;
show("sparse", () => [sparse.push(7), sparse.length, sparse[6]]);

class Stack extends Array<number> {
  top(): number {
    return this[this.length - 1];
  }
}
const stack = new Stack();
for (let i = 0; i < 5; i++) stack.push(i * 3);
show("subclass", () => [stack.length, stack.top(), stack instanceof Stack]);

const logged: string[] = [];
const proxied = new Proxy([] as number[], {
  set(target, key, value) {
    logged.push(String(key));
    return Reflect.set(target, key, value);
  },
});
proxied.push(1);
proxied.push(2);
show("proxy", () => [proxied.length, logged]);

const typed: any = new Float64Array(2);
show("typed array push", () => typed.push(1));

// A prototype index installed mid-program must reach every later push.
const before: number[] = [1];
before.push(2);
let protoHits = 0;
Object.defineProperty(Array.prototype, 3, {
  set(v: number) {
    protoHits += v;
  },
  get() {
    return undefined;
  },
  configurable: true,
});
const after: number[] = [1, 2, 3];
show("prototype setter", () => [after.push(40), protoHits, after.length]);
delete (Array.prototype as any)[3];
show("after delete", () => [before.push(5), before]);
