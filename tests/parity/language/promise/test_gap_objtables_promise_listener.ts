async function main() {
  let resolve: (n: number) => void = () => {};
  const input = new Promise<number>(r => { resolve = r; });
  const observed: number[] = [];
  const waiter = async (delta: number) => { const value = await input; observed.push(value + delta); };
  const a = waiter(11);
  const b = waiter(12);
  for (let i = 0; i < 40000; ++i) { const garbage = {i, a: [i, i + 1]}; if (garbage.i < 0) console.log(garbage); }
  resolve(37);
  await a;
  await b;
  console.log(observed.join(','));
}
main();
