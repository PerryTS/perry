// An async method whose body awaits and then returns a small integer must
// resolve to that number, also when a class with the same internal id exists
// (the raw-Int32 iter-result transport used to box it as an INT32 word, which
// decodes as the class: `[class E]` instead of 2).
class K {
  async one() { await 0; return 1; }
  async two() { await 0; return 2; }
  async neg() { await 0; return -3; }
}
const E = class { m() { return 11; } };
class F { n() { return 12; } }
async function three() { await 0; return 3; }
(async () => {
  const k = new K();
  const a = await k.one();
  const b = await k.two();
  const c = await k.neg();
  const d = await three();
  console.log(a, b, c, d);
  console.log(typeof a, typeof b, typeof c, typeof d);
  console.log(a + 1, b + 1, `${b}`, String(a), JSON.stringify([a, b, c, d]));
  console.log(b === 2, a === 1, [a, b].includes(2));
  k.two().then((v) => console.log("then", v, typeof v));
  console.log(typeof E, typeof F);
})();
