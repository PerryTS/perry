// #10048: a delegated loop emits multiple copies of the resumed scope.
// The callback's reused boxed slot must contain a cell in every copy.
async function* delegate(value: number) { yield value; }
async function* outer() {
  let base = 40;
  for (let index = 0; index < 2; index++) {
    yield* delegate(index);
    let getter = () => base + index;
    let wrapper = () => getter();
    yield wrapper();
  }
}
async function main() {
  const values: number[] = [];
  for await (const value of outer()) values.push(value);
  if (values.join(',') !== '0,40,1,41') throw new Error(values.join(','));
  console.log('PASS: delegated loop captures local callable');
}
main().catch(error => { console.error(error); process.exit(1); });
