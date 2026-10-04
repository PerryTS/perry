// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11789 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=4 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// #11789: a direct call to a closure-valued local must not hold an earlier
// argument in a register while a later argument collects.
//
// `show(a, b)` where `show` is a `const` arrow lowers through the closure-call
// arm, which evaluated the arguments left to right into bare registers. When a
// later argument runs a loop (whose back-edge polls collect) or allocates, an
// evacuating minor moves the earlier argument while the register keeps its
// old address, and the closure receives the retired from-space copy. Under the
// from-space quarantine the process faulted; without it the label printed as an
// empty string. A call to a `function` declaration rooted its arguments
// already.
//
// Each shape below passes a freshly allocated heap value (a number-to-string
// result, a concatenation, an object, an array) ahead of an argument that
// collects. Output must be byte-identical to node.

function churn(rounds: number): number {
  let n = 0;
  for (let r = 0; r < rounds; r++) {
    const a: any[] = new Array(16);
    for (let j = 0; j < 16; j++) a[j] = { j, r, s: "v" + j };
    n += a.length;
  }
  return n;
}

const show = (label: string, v: unknown) => console.log(label, String(v));
const three = (a: string, b: any, c: number) => console.log(a, JSON.stringify(b), c);
const pair = (xs: number[], o: { k: string }) => xs.join("+") + "/" + o.k;

// The original shape: a number that does not fit the small-string fast path.
const big: any = 4294967301;
show(String(big), churn(2000));

// A concatenation, then an object, each ahead of the collecting argument.
for (const seed of [2 ** 31, 2 ** 32 + 5, 1e300, -(2 ** 53)]) {
  show("seed " + String(seed), churn(500));
}
three("obj " + String(big), { k: "v" + String(big), n: [1, 2, 3] }, churn(2000));

// Both arguments allocate, and the second one collects while the first is
// held; the result is consumed after the call.
const joined = pair([big, big + 1, big + 2], { k: "k" + String(churn(2000)) });
show("pair", joined);

// The same call inside a function, so the closure is a captured binding
// rather than a module global.
function inner(seed: number): string {
  const tag = (s: string, n: number) => s + "#" + n;
  return tag("in " + String(seed * 3), churn(1000));
}
show("inner", inner(2 ** 33 + 7));
