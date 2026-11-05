// #7210, runtime half: `setInterval(fn, delay, ...args)` stores its trailing
// arguments in `IntervalTimer.args`, and `scan_timer_roots_mut` was the only
// one of its four blocks that never walked them — `CALLBACK_TIMERS` and both
// `MOCK_TIMERS` lists do. The incremental twin `scan_interval_timers_step` had
// the same hole, and cycle-based collections run ONLY the step scanner.
//
// So the object below lived in a table nothing scanned: swept at the first
// collection, then handed to the callback as a dangling pointer on the next
// tick. This is a different failure class from a stale register — it does not
// need a collection to land in a narrow window, it goes wrong at collection #0
// and stays wrong.
//
// It is also the other half of the codegen fix that ships with it: rooting an
// argument across its own lowering buys nothing if the table it lands in is not
// a root.

function churn(n: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const o = { i, pad: "y".repeat(12) };
    acc += o.i;
  }
  return acc;
}

let bad = 0;
let ticks = 0;

const payload = { tag: "interval", n: 3 };

const handle = setInterval(
  (a: { tag: string; n: number }, b: string) => {
    ticks++;
    // Read the staged arguments back on every tick. The first tick can precede
    // the first collection; the later ones cannot.
    if (a === null || typeof a !== "object" || a.tag !== "interval" || a.n !== 3) {
      bad++;
      console.log("BAD interval.a tick " + String(ticks));
    }
    if (b !== "second") {
      bad++;
      console.log("BAD interval.b tick " + String(ticks));
    }
    churn(200);
    if (ticks >= 4) {
      clearInterval(handle);
      console.log("bad " + String(bad));
    }
  },
  1,
  payload,
  "second",
);

churn(400);
