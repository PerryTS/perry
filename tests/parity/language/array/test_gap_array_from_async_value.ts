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
})();
