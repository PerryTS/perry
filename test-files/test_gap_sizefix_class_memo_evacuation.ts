// One receiver's two predicted reads straddle another receiver's slow read.
// Run with FORCE_EVACUATE + VERIFY_EVACUATION + PROTECT_FROMSPACE; the getter
// collects while the memo run's receiver handle is live.
declare function gc(): void;
class Pair { left: any = null; right: any = null; count: number = 0; }
class Trigger { value: any = 0; }
function read(p: Pair, trigger: Trigger) {
  const left = p.left;
  const value = trigger.value;
  const right = p.right;
  return left.n + value + right.n;
}
function updateThenRead(p: Pair) {
  const count = p.count++;
  const left = p.left;
  const right = p.right;
  return count + left.n + right.n;
}
let collections = 0;
for (let i = 0; i < 12; i++) {
  const p = new Pair();
  p.left = { n: i }; p.right = { n: 100 + i };
  const trigger = new Trigger();
  Object.defineProperty(trigger, "value", { get() {
    collections++;
    if (typeof gc === "function") gc();
    return 7;
  }});
  console.log(read(p, trigger), updateThenRead(p));
}
console.log("collections", collections);
