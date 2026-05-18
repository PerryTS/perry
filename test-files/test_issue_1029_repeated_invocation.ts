// Issue #1029: state-machined async fns / closures returned undefined
// on the second and subsequent calls because js_closure_alloc_with_captures_singleton
// keyed on capture-value bits and the state-machine internals weren't
// boxed — so call 2 reused call 1's terminal-state closure.

async function f(): Promise<string> {
  await Promise.resolve();
  return "hello";
}

(async () => {
  const r1 = await f();
  const r2 = await f();
  const r3 = await f();
  console.log(r1 + "," + r2 + "," + r3);
})();
