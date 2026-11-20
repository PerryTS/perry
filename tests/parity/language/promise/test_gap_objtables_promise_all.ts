async function main() {
  let resolve: (n: number) => void = () => {};
  const input = new Promise<number>(r => { resolve = r; });
  const first = Promise.all([input, 11]);
  const second = Promise.all([input, 12]);
  for (let i = 0; i < 40000; ++i) { const garbage = {i, a: [i, i + 1]}; if (garbage.i < 0) console.log(garbage); }
  resolve(37);
  console.log((await first).join(','));
  console.log((await second).join(','));
  console.log((await Promise.all([3, 4])).join(','));
}
main();
