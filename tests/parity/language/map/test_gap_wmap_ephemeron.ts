// Run the Node oracle with --expose-gc: absence of collection is a failure.
declare function gc(): void;
const ephemeronMap: any = new WeakMap();
const liveKey: any = {};
ephemeronMap.set(liveKey, { n: 41 });

function makeDeadPair() {
  const key: any = {};
  const value: any = { back: key };
  ephemeronMap.set(key, value);
  return [new WeakRef(key), new WeakRef(value)];
}
function makeDeadSelf() {
  const key: any = {};
  ephemeronMap.set(key, key);
  return new WeakRef(key);
}
function makeControl() { return new WeakRef({ control: true }); }
const probes = makeDeadPair();
const selfProbe = makeDeadSelf();
const controlProbe = makeControl();

async function checkEphemerons() {
  if (typeof gc !== "function") throw new Error("collection unavailable");
  // End the WeakRef construction job before forcing collection.
  await new Promise((resolve) => setTimeout(resolve, 0));
  for (let round = 0; round < 4; round++) {
    gc();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  const controlDead = controlProbe.deref() === undefined;
  const keyDead = probes[0].deref() === undefined;
  const valueDead = probes[1].deref() === undefined;
  const selfDead = selfProbe.deref() === undefined;
  if (!controlDead) throw new Error("weak-only control survived: vacuous collection");
  if (!keyDead || !valueDead || !selfDead) throw new Error("ephemeron retained dead cycle");
  console.log("collected", controlDead, keyDead, valueDead, selfDead);
  console.log("live", ephemeronMap.get(liveKey).n);
}
checkEphemerons();
