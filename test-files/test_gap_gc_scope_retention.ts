// Scope context objects must not over-retain: bindings are grouped by the SET
// of closures that capture them, so a surviving closure keeps only what it can
// name. `grow` alone captures `big`; `tick` alone captures `count`. Once `grow`
// is dropped, `tick` living on must not keep `big` reachable (the shared-
// context leak a single object per scope would have). Node cannot force a
// collection without --expose-gc, so there the released check is vacuous.
declare function gc(): void;
const canCollect = typeof gc === "function";

function makePair() {
  let big: number[] = new Array(200000).fill(7);
  let count = 0;
  const grow = () => {
    big = big.concat([count]);
    return big.length;
  };
  const tick = () => ++count;
  grow();
  const probe = new WeakRef(big);
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
