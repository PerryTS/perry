// #9718: a closure created INSIDE a `let`/`const` declarator's initializer may
// reference the binding that declarator introduces. The closure body does not
// run until after initialization, so node resolves it normally — only the TDZ
// window between entering the declaration and completing it is off limits.
//
// Perry pre-registered such a binding only when the reference came from an
// EARLIER statement or an EARLIER declarator of the same declaration, never
// from the declarator's own initializer, so the reference fell through to the
// unresolved-global path and threw `ReferenceError: <name> is not defined`.
// claude-code's `install` subcommand is exactly this shape:
//   let { unmount: O } = await render(el({ onDone: (a, b) => { O(), done(a, b) } }))
//
// Every declarator form is covered: plain binding, object pattern, `{ key }`
// shorthand, array pattern, nested pattern, `let` and `const`, with and
// without `await`, plus a multi-declarator statement (the earlier-declarator
// case that already worked, to pin that this did not regress).

type Handle = { u: () => string };

// Hands the callback to a later turn, then resolves — so the callback always
// runs after the declaration has completed, exactly as in the real bundle.
function render(cb: () => string): Promise<Handle> {
  return new Promise<Handle>((resolve) => {
    pending.push(cb);
    resolve({ u: () => "handle" });
  });
}
function renderSync(cb: () => string): Handle {
  pending.push(cb);
  return { u: () => "handle" };
}
const pending: (() => string)[] = [];

function report(label: string, run: () => string): void {
  try {
    console.log(label + ": " + run());
  } catch (e) {
    console.log(label + ": THREW " + ((e as Error) && (e as Error).name) + " " + ((e as Error) && (e as Error).message));
  }
}

async function main(): Promise<void> {
  // 1. plain binding, awaited initializer
  const a = await render(() => a.u() + "/plain-await");
  // 2. object pattern, awaited initializer
  const { u: b } = await render(() => b() + "/obj-await");
  // 3. object pattern, synchronous initializer
  const { u: c } = renderSync(() => c() + "/obj-sync");
  // 4. array pattern, awaited initializer
  const [d] = await render(() => d.u() + "/array-await").then((h) => [h] as [Handle]);
  // 5. `let` rather than `const`
  let { u: e } = await render(() => e() + "/let-obj-await");
  // 6. `{ key }` shorthand pattern
  const { u } = await render(() => u() + "/shorthand-await");
  // 7. nested pattern
  const { inner: { u: g } } = await Promise.resolve({ inner: { u: () => "handle" } as Handle })
    .then((v) => { pending.push(() => g() + "/nested-await"); return v; });
  // 8. multi-declarator: an EARLIER declarator's closure references a LATER one
  //    (already worked before #9718 — pinned so the reordered scan keeps it)
  const h = () => i() + "/earlier-refs-later",
    i = (): string => "handle";

  report("plain-await", () => pending[0]!());
  report("obj-await", () => pending[1]!());
  report("obj-sync", () => pending[2]!());
  report("array-await", () => pending[3]!());
  report("let-obj-await", () => pending[4]!());
  report("shorthand-await", () => pending[5]!());
  report("nested-await", () => pending[6]!());
  report("earlier-refs-later", h);

  // The shapes the pre-pass's original ordering existed for. They passed
  // before this fix and must keep passing: moving the initializer scan earlier
  // is a superset, not a replacement.
  const fact = (n: number): number => (n <= 1 ? 1 : n * fact(n - 1));
  const fib = function rec(n: number): number { return n < 2 ? n : rec(n - 1) + rec(n - 2); };
  const off = renderSync(() => off.u() + "/init-call-result");
  console.log("self-recursive-arrow=" + fact(5));
  console.log("named-fn-expr-recursion=" + fib(10));
  report("init-call-result", () => pending[7]!());

  // The declaration is complete by the time these run, so the direct calls
  // must agree with what the closures saw.
  report("direct-plain", () => a.u());
  report("direct-obj", () => b());
  report("direct-array", () => d.u());
  report("direct-nested", () => g());
  console.log("count=" + pending.length + " c=" + c() + " e=" + e() + " u=" + u() + " i=" + i());
}

void main();
