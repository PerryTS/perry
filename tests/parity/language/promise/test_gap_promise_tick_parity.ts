// Microtask hop-count harness: a metronome chain logs tick0..tickN while each
// promise shape under test logs its settlement. The tick number that appears
// immediately before the shape's log = its hop count. Any node-vs-perry
// difference in ANY shape = an ordering divergence.
function run(label, arm) {
  return new Promise((done) => {
    const order = [];
    let p = Promise.resolve();
    for (let i = 0; i < 8; i++) {
      const n = i;
      p = p.then(() => { order.push("t" + n); });
    }
    const metroDone = p;
    arm(() => order.push("X"));
    // Deterministic: the metronome outlives every shape's cascade; snapshot
    // after it completes + one macrotask backstop.
    metroDone.then(() => setTimeout(() => { done(label.padEnd(34) + order.join(" ")); }, 0));
  });
}

(async () => {
  const out = [];
  out.push(await run("then-on-resolved", (X) => {
    Promise.resolve(1).then(X);
  }));
  out.push(await run("asyncfn-return-value", (X) => {
    (async () => 1)().then(X);
  }));
  out.push(await run("asyncfn-return-resolved-promise", (X) => {
    (async () => Promise.resolve(1))().then(X);
  }));
  out.push(await run("executor-resolve-with-promise", (X) => {
    new Promise((res) => res(Promise.resolve(1))).then(X);
  }));
  out.push(await run("promise-all-2-resolved", (X) => {
    Promise.all([Promise.resolve(1), Promise.resolve(2)]).then(X);
  }));
  out.push(await run("promise-all-then-chain", (X) => {
    Promise.all([Promise.resolve(1), Promise.resolve(2)]).then(() => 1).then(X);
  }));
  out.push(await run("finally-then", (X) => {
    Promise.resolve(1).finally(() => {}).then(X);
  }));
  out.push(await run("finally-orig-then", (X) => {
    const r = Promise.resolve(1);
    r.finally(() => {});
    r.then(X);
  }));
  out.push(await run("await-then (async wrapper)", (X) => {
    (async () => { await Promise.resolve(1); })().then(X);
  }));
  out.push(await run("thenable-assimilation", (X) => {
    new Promise((res) => res({ then(r) { r(1); } })).then(X);
  }));
  out.push(await run("all-of-asyncfn-results", (X) => {
    const f = async () => 1;
    Promise.all([f(), f()]).then(X);
  }));
  console.log(out.join("\n"));
})();
