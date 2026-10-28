// parity-env: PERRY_REALM_TEST_FIXTURE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_SCHEDULE_SEED=1301 PERRY_GC_SCHEDULE_RATE=0.2 PERRY_GC_SCHEDULE_ALLOC_KB=64
// WRT step 1: GC-headed cells in the realm image (`perry_realm`).
// PERRY_REALM_TEST_FIXTURE makes the compiler emit one function cell (body:
// the runtime's fixture, returns 42) and one `{value, done}` object whose
// `value` slot names it, both as static data in the image section, and
// publish the object as globalThis.__perryRealmFixture. The program then
// (a) reads through the image across collections, (b) stores heap objects
// into an image slot under moving collections, and (c) adds keys (shape
// transitions, overflow storage on the heap). Forced evacuation moves every
// survivor, the schedule seed adds collections at random safepoints, and the
// evacuation verifier checks image cells every cycle. Node has no fixture, so
// this test carries its own expected output (test-parity/expected/).
const fx: any = (globalThis as any).__perryRealmFixture;
const collect: any = (globalThis as any).gc;

function churn(n: number): number {
  const a: any[] = [];
  for (let i = 0; i < n; i++) a.push({ i, s: "x" + i });
  return a.length;
}

if (fx === undefined) {
  console.log("no realm fixture");
} else {
  let a = 0;
  for (let r = 0; r < 20; r++) {
    churn(20000);
    if (typeof collect === "function") collect();
    if (fx.value() === 42 && fx.done === false) a++;
  }
  console.log("a", a);

  for (let r = 0; r < 20; r++) {
    fx.done = { round: r, list: [r, r + 1, "s" + r] };
    churn(20000);
    if (typeof collect === "function") collect();
  }
  console.log("b", JSON.stringify(fx.done));

  for (let k = 0; k < 30; k++) fx["k" + k] = "v" + k + "-" + "y".repeat(k);
  churn(50000);
  if (typeof collect === "function") collect();
  let c = 0;
  for (let k = 0; k < 30; k++) if (fx["k" + k] === "v" + k + "-" + "y".repeat(k)) c++;
  console.log("c", c, Object.keys(fx).length, fx.value());
}
