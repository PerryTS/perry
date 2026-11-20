// LIVE tee: producer enqueues AFTER tee() (like the RSC flush) — chunks must
// travel the demand-pull cycle, never eagerly pre-fill branch queues.
function metro(order, n) { let p = Promise.resolve(); for (let i = 0; i < n; i++) { const k = i; p = p.then(() => order.push("t" + k)); } }
(async () => {
  const order = [];
  const enc = new TextEncoder();
  let ctrl;
  const src = new ReadableStream({ type: "bytes", start(c) { ctrl = c; } });
  const [a, b] = src.tee();
  metro(order, 14);
  setImmediate(() => { ctrl.enqueue(enc.encode("aa")); ctrl.enqueue(enc.encode("bb")); ctrl.close(); order.push("FLUSHED"); });
  const ra = a.getReader();
  const aDone = new Promise((fin) => {
    (function pa({ done, value } = {}) { if (done) { order.push("Adone"); fin(); return; } if (value) order.push("x" + new TextDecoder().decode(value)); return ra.read().then(pa); })();
  });
  const rb = b.getReader();
  const bDone = new Promise((fin) => {
    (function pb({ done, value } = {}) { if (done) { order.push("Bdone"); fin(); return; } if (value) order.push("r" + new TextDecoder().decode(value)); return rb.read().then(pb); })();
  });
  // Deterministic completion: both pumps saw done + one macrotask backstop.
  await aDone; await bDone;
  await new Promise((res) => setTimeout(res, 0));
  console.log("livetee  " + order.join(" "));
})();
