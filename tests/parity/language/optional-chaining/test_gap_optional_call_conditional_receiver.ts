// An optional call whose RECEIVER is a conditional expression must
// short-circuit like any other: `(c ? o : undefined)?.write(x)` is `undefined`
// when the ternary yields nullish, and the real call when it does not.
//
// Perry's optional-chain call lowering destructured any `Expr::Conditional`
// receiver as if a `?.` had produced it, reading the ternary's own condition
// and then-branch as the short-circuit test. The result was
// `c ? o : undefined.write(x)` — the RECEIVER came back when the condition
// held, and a `TypeError: Cannot read properties of undefined (reading
// 'write')` when it did not.
//
// Found via claude-code, whose first `process.on("exit")` listener is exactly
// this shape:
//   (process.stderr.isTTY ? process.stderr : process.stdout.isTTY
//      ? process.stdout : void 0)?.write(resetSequence)
// With both streams piped that value is `undefined`, so the listener threw,
// the throw escaped `process.exit()`, and claude-code's
// `try { process.exit(q) } catch { process.kill(process.pid, "SIGKILL") }`
// fallback killed the process mid-flush.
const c = false;
const o: any = { write: (s: string) => "wrote:" + s };

function probe(label: string, f: () => unknown): void {
  try {
    console.log(label, "=>", f());
  } catch (e: unknown) {
    console.log(label, "=> THREW", (e as Error)?.message);
  }
}

// Nullish tails, in every spelling and nesting depth.
probe("void0 alone       ", () => (void 0 as any)?.write("x"));
probe("tern undefined    ", () => (c ? o : undefined)?.write("x"));
probe("tern void0        ", () => (c ? o : (void 0 as any))?.write("x"));
probe("tern null         ", () => (c ? o : null)?.write("x"));
probe("nested undefined  ", () => (c ? o : c ? o : undefined)?.write("x"));
probe("nested void0      ", () => (c ? o : c ? o : (void 0 as any))?.write("x"));
probe("nested null       ", () => (c ? o : c ? o : null)?.write("x"));
probe("deep nested       ", () => (c ? o : c ? o : c ? o : undefined)?.write("x"));

// Non-nullish results must still CALL the method, not hand back the receiver.
probe("nested tail object", () => (c ? o : c ? o : o)?.write("x"));
probe("first arm taken   ", () => (!c ? o : c ? o : undefined)?.write("x"));
probe("tern object       ", () => (!c ? o : undefined)?.write("x"));

// Via a temporary — the shape that always worked; a control.
probe("through a local   ", () => {
  const v: any = c ? o : c ? o : undefined;
  return v?.write("x");
});

// Property read (no call) on the same receivers — the other control.
probe("prop read nullish ", () => (c ? o : c ? o : undefined)?.write);
probe("prop read object  ", () => typeof (!c ? o : undefined)?.write);

// The upstream-chain shape this branch actually exists for must keep working:
// a receiver that IS an optional chain still short-circuits without calling.
const holder: any = { inner: { m: (s: string) => "inner:" + s } };
const empty: any = {};
probe("chain receiver hit", () => holder.inner?.m("x"));
probe("chain receiver mis", () => empty.inner?.m("x"));
probe("chain then member ", () => empty?.inner?.m("x"));
probe("chain optional call", () => empty?.inner?.m?.("x"));
probe("chain deep hit    ", () => holder?.inner?.m?.("x"));

// The real-world receiver: a stream picked by a ternary over `isTTY`.
const picked: any = process.stderr.isTTY
  ? process.stderr
  : process.stdout.isTTY
    ? process.stdout
    : void 0;
console.log("picked typeof:", typeof picked);
probe("isTTY ternary     ", () =>
  (process.stderr.isTTY
    ? process.stderr
    : process.stdout.isTTY
      ? process.stdout
      : (void 0 as any)
  )?.write(""),
);
