import * as net from "node:net";
const variant = process.argv[2] || "queueMicrotask"; const N = Number(process.argv[3] || "1000");
const defer: (f: () => void) => void =
  variant === "direct" ? (f) => f() : variant === "queueMicrotask" ? (f) => queueMicrotask(f)
  : variant === "promise" ? (f) => { Promise.resolve().then(f); } : variant === "nextTick" ? (f) => process.nextTick(f)
  : (f) => { setImmediate(f); };
const server = net.createServer((c) => { c.setNoDelay(true); c.on("data", (d) => c.write(d)); });
server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as net.AddressInfo).port;
  const s = new net.Socket(); s.setNoDelay(true);
  let i = 0; let bytes = 0; let t0 = 0;
  const send = () => s.write("ping" + i);
  s.on("data", (d) => {
    bytes += d.length;
    if (++i === N) {
      const ms = performance.now() - t0;
      console.log(`variant=${variant} N=${N} checksum=${N} ms=${ms.toFixed(1)} rtt_per_s=${(N / ms * 1000).toFixed(0)}`);
      s.destroy(); server.close();
    } else defer(send);
  });
  s.connect(port, "127.0.0.1", () => { t0 = performance.now(); send(); });
});