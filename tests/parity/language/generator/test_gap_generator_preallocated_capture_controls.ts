// #10048 controls: fresh iteration cells, empty delegates, TDZ, var identity,
// recursive closures, and captures made after ordinary yield/await.
function check(label: string, actual: string, expected: string) {
  if (actual !== expected) throw new Error(label + ': ' + actual + ' != ' + expected);
  console.log(label + ': ' + actual);
}
async function* emptyDelegate() { if (false) yield -1; }
async function* nonemptyDelegate(value: number) { yield value; }
async function* emptyLoop() {
  for (let index = 0; index < 2; index++) {
    yield* emptyDelegate();
    let getter = () => index + 40;
    let wrapper = () => getter();
    yield wrapper();
  }
}
async function* yieldControl() {
  yield 1;
  let getter = () => 42;
  let wrapper = () => getter();
  yield wrapper();
}
async function awaitControl() {
  await Promise.resolve(1);
  let getter = () => 42;
  let wrapper = () => getter();
  return wrapper();
}
async function* retained(callbacks: Array<() => number>) {
  for (let index = 0; index < 3; index++) {
    yield* nonemptyDelegate(index);
    let value = index + 10;
    let getter = () => value;
    let wrapper = () => getter();
    callbacks.push(wrapper);
    value += 100;
    yield wrapper();
  }
}
// Separate invocations: repeated-block TDZ reset already fails on main (#10051).
function tdzAndRecursion(index: number) {
  const results: string[] = [];
  let read = () => value;
  try { read(); results.push('missing-tdz'); }
  catch (error) { results.push(error instanceof ReferenceError ? 'tdz' : 'wrong-error'); }
  let value: number;
  results.push(String(read()));
  value = index;
  results.push(String(read()));
  let recurse = (n: number): number => n === 0 ? value : recurse(n - 1);
  results.push(String(recurse(2)));
  return results.join(',');
}
function hoistedVar() {
  const callbacks: Array<() => number> = [];
  for (let index = 0; index < 3; index++) {
    var value = index;
    callbacks.push(() => value);
  }
  return callbacks.map(callback => callback()).join(',');
}
async function collect(iterator: AsyncIterable<number>) {
  const values: number[] = [];
  for await (const value of iterator) values.push(value);
  return values.join(',');
}
async function main() {
  check('empty delegate', await collect(emptyLoop()), '40,41');
  check('ordinary yield', await collect(yieldControl()), '1,42');
  check('ordinary await', String(await awaitControl()), '42');
  const callbacks: Array<() => number> = [];
  check('retained yields', await collect(retained(callbacks)), '0,110,1,111,2,112');
  check('retained callbacks', callbacks.map(callback => callback()).join(','), '110,111,112');
  check('TDZ and recursion', tdzAndRecursion(0) + ',' + tdzAndRecursion(1), 'tdz,undefined,0,0,tdz,undefined,1,1');
  check('hoisted var', hoistedVar(), '2,2,2');
  console.log('PASS: preallocated capture controls');
}
main().catch(error => { console.error(error); process.exit(1); });
