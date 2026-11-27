async function main() {
  const fns: (() => number)[] = [];
  for (let [key, { value }] of new Map([["a", {value: 1}], ["b", {value: 2}]])) {
    fns.push(() => ++value);
    await Promise.resolve();
  }
  console.log(fns[0](), fns[1](), fns[0]());
}
main().catch(e => { console.error(e); process.exitCode = 1; });
