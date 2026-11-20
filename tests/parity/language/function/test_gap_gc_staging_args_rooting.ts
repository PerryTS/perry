// #7210 §2: an argument staging buffer filled INTERLEAVED with lowering.
//
// `setTimeout(cb, 0, {…}, churn())` lowers to
//
//     %buf = alloca [2 x double]                 ; a bare entry alloca
//     %v0  = <lower {…}>                          ; a fresh heap object
//     store double %v0, ptr %buf[0]               ; ← now the ONLY reference
//     %v1  = call double @churn()                 ; loop back-edge poll -> moving minor
//     store double %v1, ptr %buf[1]
//     call void @js_set_timeout_callback_args(…, ptr %buf, …)
//
// `%buf` is neither a shadow slot nor a temp root, so the precise root walk
// never sees `%v0`. The window is not staleness — it is a premature SWEEP: at
// the moment `churn()` collects, nothing anywhere refers to the object.
//
// The same shape covers setImmediate / setInterval / process.nextTick / the
// `timers` namespace forms / a spread call's regular-argument buffer.

function churn(n: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const o = { i, pad: "x".repeat(8) };
    acc += o.i;
  }
  return acc;
}

let bad = 0;

function expect(label: string, got: unknown, want: unknown): void {
  if (got !== want) {
    bad++;
    console.log("BAD " + label + ": got " + String(got) + " want " + String(want));
  }
}

// --- setTimeout with trailing args -----------------------------------------
setTimeout(
  (a: { tag: string; n: number }, b: number) => {
    expect("setTimeout.a.tag", a.tag, "alpha");
    expect("setTimeout.a.n", a.n, 11);
    expect("setTimeout.b", b, 4950);
  },
  0,
  { tag: "alpha", n: 11 },
  churn(100),
);

// --- setImmediate with trailing args ---------------------------------------
setImmediate(
  (a: { tag: string }, b: { tag: string }, c: number) => {
    expect("setImmediate.a", a.tag, "beta");
    expect("setImmediate.b", b.tag, "gamma");
    expect("setImmediate.c", c, 4950);
  },
  { tag: "beta" },
  { tag: "gamma" },
  churn(100),
);

// --- process.nextTick with trailing args -----------------------------------
process.nextTick(
  (a: { tag: string }, b: number) => {
    expect("nextTick.a", a.tag, "delta");
    expect("nextTick.b", b, 4950);
  },
  { tag: "delta" },
  churn(100),
);

// --- a spread call's regular-argument buffer -------------------------------
const rest = [7, 8];
const sink = (a: { tag: string }, b: number, ...more: number[]): string =>
  a.tag + ":" + String(b) + ":" + more.join(",");
expect("spread", sink({ tag: "eps" }, churn(100), ...rest), "eps:4950:7,8");

setTimeout(() => {
  console.log("bad " + String(bad));
}, 1);
