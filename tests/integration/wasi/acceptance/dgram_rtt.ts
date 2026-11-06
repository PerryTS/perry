// The WASI event loop wakes on UDP readiness: 200 ping round trips to a
// host-side echo peer (_helpers/echo_peer.py) that answers each ping 0.2 ms
// after it arrives finish in under 150 ms. The delay makes the guest idle when
// the reply lands, so a loop that sleeps 1 ms per idle park instead of waiting
// on the socket takes at least 200 ms.
import * as dgram from "node:dgram";

const port = Number(process.argv[2]);
const WARMUP = 20;
const N = 200;
const socket = dgram.createSocket("udp4");
let got = 0;
let start = 0;
let intact = true;

function ping(i: number): void {
  const msg = Buffer.alloc(16, i & 255);
  socket.send(msg, 0, msg.length, port, "127.0.0.1");
}
socket.on("message", (msg) => {
  if (msg.length !== 16 || msg[0] !== (got & 255)) intact = false;
  got++;
  if (got === WARMUP) start = performance.now();
  if (got === WARMUP + N) {
    const ms = performance.now() - start;
    console.log(`${N} round trips intact ${intact}`);
    console.log(`under 150 ms ${ms < 150}`);
    if (ms >= 150) console.error(`took ${ms.toFixed(1)} ms`);
    socket.close();
    return;
  }
  ping(got);
});
socket.bind(0, "127.0.0.1", () => ping(0));
