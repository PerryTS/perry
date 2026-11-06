// #1671: `setTimeout(fn)` called with a single argument and no delay.
// Node treats the missing delay as 0. Before the fix, the global 1-arg
// form fell through codegen's extern-func table to a bare `@setTimeout`
// call and the binary failed to LINK (`Undefined symbols: _setTimeout`) —
// the exact failure hit by hono/jsx's `hooks/index.js`, which schedules a
// re-render via `setTimeout(() => { … })`. Uses the bare global (no
// `node:timers` import) to exercise that path directly.
const order: string[] = [];
await new Promise<void>((resolve) => {
  setTimeout(() => {
    order.push("no-delay");
    resolve();
  });
});
console.log("fired:", order.join(","));
