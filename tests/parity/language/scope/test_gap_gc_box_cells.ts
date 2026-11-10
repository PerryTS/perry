// Mutable cells must survive relocation through both frame roots and captures.
// Run with forced evacuation and with both native and shadow roots.
declare function gc(): void;
function collect() {
  if (typeof gc === "function") gc();
}

function counter(seed: number) {
  let n = seed;
  let payload = { value: seed };
  return {
    read: () => { collect(); return n + payload.value; },
    bump: () => { collect(); n++; payload = { value: n }; return n; },
    child: () => () => { collect(); return ++n; },
  };
}
const c = counter(4);
console.log(c.read(), c.bump(), c.read(), c.child()(), c.read());

function coercion() {
  let n: any = { valueOf() { collect(); return 40; } };
  const bump = () => ++n;
  console.log(bump(), bump());
}
coercion();

function loopCells() {
  const callbacks: Array<() => number> = [];
  for (let i = 0; i < 20; i++) {
    let value = i;
    callbacks.push(() => ++value);
    collect();
  }
  let sum = 0;
  for (const callback of callbacks) sum += callback();
  console.log(sum);
}
loopCells();

async function activation(seed: number) {
  let n = seed;
  const inc = () => ++n;
  await Promise.resolve(0);
  collect();
  inc();
  await Promise.resolve(0);
  collect();
  return inc;
}
async function run() {
  const one = await activation(10);
  const two = await activation(20);
  collect();
  console.log(one(), two(), one());
}
run();
