// Array.prototype.flatMap reached with a receiver that is not a plain array:
// a Proxy over an array (Solid store drafts), an array-like, and a borrowed
// reference. The generic path used to be a noop that returned a non-array.
const target: any[] = [{ a: 1 }, { a: 2, mention: { text: "x" } }, 3];
const proxy: any = new Proxy(target, { get: (t, p, r) => Reflect.get(t, p, r) });

const fromProxy = proxy.flatMap((v: any) => (typeof v === "number" ? [v, [v]] : [v.a]));
console.log(Array.isArray(fromProxy), JSON.stringify(fromProxy));
console.log(new Map(fromProxy.map((v: any, i: number) => [i, v])).size);

const arrayLike = { length: 3, 0: "a", 2: "c" };
console.log(JSON.stringify(Array.prototype.flatMap.call(arrayLike, (v: string, i: number) => [v, i])));

const nestedProxy = new Proxy([1, 2], {});
console.log(JSON.stringify([0].flatMap(() => nestedProxy)));
console.log(JSON.stringify(Array.prototype.flatMap.call(proxy, function (this: any, v: any) { return this.k; }, { k: ["t"] })));

const borrowed = { length: 2, 0: 5, 1: 6, flatMap: Array.prototype.flatMap };
console.log(JSON.stringify(borrowed.flatMap((v: number) => [v, v * 2])));
console.log(Array.prototype.flatMap.length, Array.prototype.flatMap.name);

// thisArg binds the mapper's `this` on plain arrays too.
const local = [1, 2];
console.log(JSON.stringify(local.flatMap(function (this: any, v: number) { return [this.k, v]; }, { k: "l" })));
console.log(JSON.stringify([7].flatMap(function (this: any) { return this.k; }, { k: "lit" })));
try {
  proxy.flatMap(undefined);
} catch (e) {
  console.log((e as Error).constructor.name);
}
