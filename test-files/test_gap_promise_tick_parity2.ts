// Extended hop shapes: the ones the app's module chain actually uses that
// hops.js round 1 did NOT cover. Any node/perry difference = the third leg.
function run(label, arm) {
  return new Promise((done) => {
    const order = [];
    let p = Promise.resolve();
    for (let i = 0; i < 10; i++) { const n = i; p = p.then(() => order.push("t" + n)); }
    const metroDone = p;
    arm((tag) => order.push(tag));
    metroDone.then(() => setTimeout(() => { done(label.padEnd(30) + order.join(" ")); }, 0));
  });
}
const later = () => { let r; const p = new Promise((res) => { r = res; }); Promise.resolve().then(() => r(1)); return p; };

(async () => {
  const out = [];
  out.push(await run("all-of-pending", (X) => {
    Promise.all([later(), later()]).then(() => X("X"));
  }));
  out.push(await run("finally-on-pending", (X) => {
    later().finally(() => {}).then(() => X("X"));
  }));
  out.push(await run("finally-orig-pending", (X) => {
    const r = later(); r.finally(() => {}); r.then(() => X("X"));
  }));
  out.push(await run("two-thens-same-promise", (X) => {
    const p = Promise.resolve(1);
    p.then(() => X("A")); p.then(() => X("B"));
  }));
  out.push(await run("two-thens-pending", (X) => {
    const p = later();
    p.then(() => X("A")); p.then(() => X("B"));
  }));
  out.push(await run("then-bound", (X) => {
    const s = new Set(); const p = Promise.resolve(7);
    p.then(s.add.bind(s, p), () => {}); p.then(() => X("X"));
  }));
  out.push(await run("all-then-require-chain", (X) => {
    // d()'s exact tail: Promise.all(r).then(() => sync()) feeding a dep .then
    const dep = Promise.all([later(), later()]).then(() => 42);
    dep.then(() => X("X"));
  }));
  console.log(out.join("\n"));
})();
