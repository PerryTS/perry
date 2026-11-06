// Web Streams hop harness: a source stream with 3 chunks already written and
// closed (like the completed RSC output), teed; branch-2's chained read loop
// (like the flight pump) races a metronome chain. The tick at which each read
// callback fires must match node exactly — any divergence reorders the app.
function metro(order, n) {
  let p = Promise.resolve();
  for (let i = 0; i < n; i++) { const k = i; p = p.then(() => order.push("t" + k)); }
}

async function scenario(label, useTee) {
  const order = [];
  const enc = new TextEncoder();
  const src = new ReadableStream({
    start(c) { c.enqueue(enc.encode("aa")); c.enqueue(enc.encode("bb")); c.enqueue(enc.encode("cc")); c.close(); },
  });
  let stream = src;
  let other = null;
  if (useTee) { const [a, b] = src.tee(); other = a; stream = b; }
  const metroDone = (function metro2(n) { let p = Promise.resolve(); for (let i = 0; i < n; i++) { const k = i; p = p.then(() => order.push("t" + k)); } return p; })(12);
  const dones = [];
  const reader = stream.getReader();
  dones.push(new Promise((fin) => {
    (function pump({ done, value } = {}) {
      if (done) { order.push("DONE"); fin(); return; }
      if (value) order.push("r" + new TextDecoder().decode(value));
      return reader.read().then(pump);
    })({ done: false });
  }));
  // drain the other branch too (the app reads both tee branches)
  if (other) {
    const r2 = other.getReader();
    dones.push(new Promise((fin) => {
      (function pump2({ done, value } = {}) {
        if (done) { order.push("done2"); fin(); return; }
        if (value) order.push("x" + new TextDecoder().decode(value));
        return r2.read().then(pump2);
      })({ done: false });
    }));
  }
  // Deterministic completion: all pumps finished + metronome drained + one
  // macrotask so trailing nextTick-deferred events (tee close) land.
  await Promise.all(dones); await metroDone;
  await new Promise((res) => setTimeout(res, 0));
  console.log(label.padEnd(12) + order.join(" "));
}

(async () => {
  await scenario("plain", false);
  await scenario("tee", true);
})();
