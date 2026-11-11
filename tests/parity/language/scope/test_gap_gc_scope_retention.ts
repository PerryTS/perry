// Scope records follow lexical activations. A surviving callback can retain
// sibling slots in its own scope, but must not retain a separate inner scope
// that it never captures. Node cannot force collection without --expose-gc,
// so there the released check is vacuous.
declare function gc(): void;
const canCollect = typeof gc === "function";

function makePair() {
  let count = 0;
  const tick = () => ++count;
  let probe: WeakRef<number[]>;
  {
    let big: number[] = new Array(200000).fill(7);
    const grow = () => {
      big = big.concat([count]);
      return big.length;
    };
    grow();
    probe = new WeakRef(big);
  }
  return { tick, probe };
}

function makeKeeper() {
  let big: number[] = new Array(50000).fill(3);
  let reads = 0;
  const keep = () => {
    reads++;
    return big.length + reads;
  };
  big = big.slice(1);
  const probe = new WeakRef(big);
  return { keep, probe };
}

function churn() {
  let junk: any[] = [];
  for (let i = 0; i < 20000; i++) {
    junk.push({ i });
    if (junk.length > 100) junk = [];
  }
}

async function main() {
  const pair = makePair();
  const keeper = makeKeeper();
  await new Promise((r) => setTimeout(r, 0));
  for (let i = 0; i < 4; i++) {
    churn();
    if (canCollect) gc();
    await Promise.resolve();
  }
  const released = canCollect ? pair.probe.deref() === undefined : true;
  console.log("unrelated binding released:", released);
  console.log("captured binding kept:", keeper.probe.deref() !== undefined, keeper.keep());
  console.log("survivor still works:", pair.tick(), pair.tick());
}
main();
