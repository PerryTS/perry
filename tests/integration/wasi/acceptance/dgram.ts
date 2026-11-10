// node:dgram over the WASI reactor: two loopback sockets echo 1000 datagrams
// (2..1200 bytes) in a window of 8; every datagram arrives, in order per pair,
// with Node's rinfo, and every send callback reports its length.
import * as dgram from "node:dgram";

const N = 1000;
const WINDOW = 8;
const a = dgram.createSocket({ type: "udp4", reuseAddr: true });
const b = dgram.createSocket({ type: "udp4", reuseAddr: true });

function size(i: number): number { return 2 + ((i * 397) % 1199); }
function payload(i: number): Buffer {
  const buf = Buffer.alloc(size(i), (i * 7) & 255);
  buf[0] = i >> 8;
  buf[1] = i & 255;
  return buf;
}
function intact(msg: Buffer, i: number): boolean {
  if (msg.length !== size(i) || msg[0] !== (i >> 8) || msg[1] !== (i & 255)) return false;
  for (let k = 2; k < msg.length; k++) if (msg[k] !== ((i * 7) & 255)) return false;
  return true;
}

let listening = 0, closed = 0, sent = 0, gotA = 0, gotB = 0;
let callbacks = 0, cbErrors = 0, queued = 0, bytesA = 0, bytesB = 0;
let orderA = true, orderB = true, rinfoOk = true;

function seq(msg: Buffer): number { return (msg[0] << 8) | msg[1]; }
function onSent(err: Error | null, bytes: number, want: number): void {
  callbacks++;
  if (err || bytes !== want) cbErrors++;
  queued = Math.max(queued, a.getSendQueueCount(), b.getSendQueueCount());
}
function sendNext(): void {
  const i = sent++;
  const msg = payload(i);
  a.send(msg, 0, msg.length, b.address().port, "127.0.0.1", (err, n) => onSent(err, n, msg.length));
}

b.on("message", (msg, rinfo) => {
  if (seq(msg) !== gotB || !intact(msg, gotB)) orderB = false;
  if (rinfo.address !== "127.0.0.1" || rinfo.family !== "IPv4" ||
      rinfo.port !== a.address().port || rinfo.size !== msg.length) rinfoOk = false;
  gotB++;
  bytesB += msg.length;
  b.send(msg, 0, msg.length, rinfo.port, rinfo.address, (err, n) => onSent(err, n, msg.length));
});
a.on("message", (msg, rinfo) => {
  if (seq(msg) !== gotA || !intact(msg, gotA)) orderA = false;
  if (rinfo.port !== b.address().port) rinfoOk = false;
  gotA++;
  bytesA += msg.length;
  if (sent < N) sendNext();
  if (gotA === N) { a.close(); b.close(); }
});
for (const s of [a, b]) {
  s.on("error", (err) => { console.log("error", err.message); process.exit(1); });
  s.on("close", () => {
    if (++closed < 2) return;
    console.log("b received", gotB, "in order", orderB, "bytes", bytesB);
    console.log("a received", gotA, "in order", orderA, "bytes", bytesA);
    console.log("send callbacks", callbacks, "errors", cbErrors, "queue", queued);
    console.log("rinfo ok", rinfoOk);
    console.log("closed", closed);
  });
  s.on("listening", () => {
    const addr = s.address();
    console.log("listening", addr.address, addr.family, addr.port > 0);
    if (++listening === 2) for (let k = 0; k < WINDOW; k++) sendNext();
  });
}
a.bind(0, "127.0.0.1");
b.bind(0, "127.0.0.1");
