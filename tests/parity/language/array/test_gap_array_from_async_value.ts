// Array.fromAsync is an own function of the Array constructor (ECMA-262
// 23.1.2.2), read as a value and called directly.
const fromAsync = (Array as any).fromAsync;
console.log(typeof fromAsync, fromAsync.name, fromAsync.length);
console.log(Object.getOwnPropertyNames(Array).includes("fromAsync"));

(async () => {
  console.log(JSON.stringify(await Array.fromAsync([1, 2])));
  console.log(JSON.stringify(await fromAsync([3, Promise.resolve(4)])));
  console.log(JSON.stringify(await fromAsync.call(undefined, [5], (x: number) => x * 2)));
  async function* gen() {
    yield 6;
    yield 7;
  }
  console.log(JSON.stringify(await Array.fromAsync(gen())));

  // A constructor receiver builds the result (proposal steps 3.j.ii, 3.k.iv).
  class Bag {
    args: unknown[];
    constructor(...args: unknown[]) {
      this.args = args;
    }
  }
  const fromIterable: any = await fromAsync.call(Bag, gen());
  console.log(fromIterable instanceof Bag, JSON.stringify(fromIterable.args), fromIterable.length, fromIterable[0], fromIterable[1]);
  const fromArrayLike: any = await fromAsync.call(Bag, { length: 2, 0: "a", 1: "b" });
  console.log(fromArrayLike instanceof Bag, JSON.stringify(fromArrayLike.args), fromArrayLike.length, fromArrayLike[1]);
  class List extends Array {}
  const list = await (List as any).fromAsync([8, 9]);
  console.log(list instanceof List, Array.isArray(list), JSON.stringify(list));
  console.log(Array.isArray(await fromAsync.call({}, [1])));

  // The result's own definitions and length write fail loudly.
  function Sealed() {
    return Object.preventExtensions([]);
  }
  await fromAsync.call(Sealed, [1]).then(
    () => console.log("sealed resolved"),
    (e: any) => console.log("sealed rejected", e instanceof TypeError),
  );
  function FixedLength() {
    return Object.defineProperty([], "length", { writable: false });
  }
  await fromAsync.call(FixedLength, []).then(
    () => console.log("fixed length resolved"),
    (e: any) => console.log("fixed length rejected", e instanceof TypeError),
  );
})();
