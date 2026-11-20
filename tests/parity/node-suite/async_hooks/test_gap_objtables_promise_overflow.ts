import {AsyncLocalStorage} from 'node:async_hooks';
async function main() {
  const als = new AsyncLocalStorage<number>();
  let resolve: (n: number) => void = () => {};
  const input = new Promise<number>(r => { resolve = r; });
  const observed: string[] = [];
  als.run(11, () => { input.then(v => { observed.push('a:' + v + ':' + als.getStore()); }); });
  als.run(12, () => { input.then(v => { observed.push('b:' + v + ':' + als.getStore()); }); });
  als.run(13, () => { input.then(v => { observed.push('c:' + v + ':' + als.getStore()); }); });
  for (let i = 0; i < 40000; ++i) { const garbage = {i, a: [i, i + 1]}; if (garbage.i < 0) console.log(garbage); }
  resolve(37);
  await input;
  await Promise.resolve();
  console.log(observed.join(','));
  console.log(als.getStore() === undefined);
}
main();
