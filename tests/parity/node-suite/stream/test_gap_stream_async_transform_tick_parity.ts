function metro(order, n) { let p = Promise.resolve(); for (let i = 0; i < n; i++) { const k = i; p = p.then(() => order.push("t" + k)); } }
(async () => {
  const order = [];
  const enc = new TextEncoder();
  const src = new ReadableStream({ start(c) { c.enqueue(enc.encode("aa")); c.enqueue(enc.encode("bb")); c.close(); } });
  let buf = [];
  const ts = new TransformStream({
    transform(ch, ctrl) {
      buf.push(ch);
      return new Promise((res) => setImmediate(() => { for (const b of buf) ctrl.enqueue(b); buf = []; res(); }));
    },
  });
  const out = src.pipeThrough(ts);
  metro(order, 10);
  const ro = out.getReader();
  const aDone = new Promise((fin) => {
    (function pumpA({ done, value } = {}) { if (done) { order.push("adone"); fin(); return; } if (value) order.push("w" + new TextDecoder().decode(value)); return ro.read().then(pumpA); })();
  });
  // Deterministic completion: the pump saw done + one macrotask backstop.
  await aDone;
  await new Promise((res) => setTimeout(res, 0));
  console.log("asynctf  " + order.join(" "));
})();
