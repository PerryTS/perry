// The WASI event loop wakes on TCP readiness: 200 ping round trips to a
// host-side echo peer (_helpers/echo_peer.py) that answers each ping 0.2 ms
// after it arrives finish in under 150 ms. The delay makes the guest idle when
// the reply lands, so a loop that sleeps 1 ms per idle park instead of waiting
// on the socket takes at least 200 ms.
import * as net from "node:net";

const port = Number(process.argv[3]);
const WARMUP = 20;
const N = 200;
let got = 0;
let start = 0;
let pending = 0;
let intact = true;

const client = net.createConnection(port, "127.0.0.1", () => {
  client.setNoDelay(true);
  ping();
});
function ping(): void {
  client.write(Buffer.alloc(16, got & 255));
}
client.on("data", (data) => {
  for (let k = 0; k < data.length; k++) {
    if (data[k] !== (got & 255)) intact = false;
    if (++pending < 16) continue;
    pending = 0;
    got++;
    if (got === WARMUP) start = performance.now();
    if (got === WARMUP + N) {
      const ms = performance.now() - start;
      console.log(`${N} round trips intact ${intact}`);
      console.log(`under 150 ms ${ms < 150}`);
      if (ms >= 150) console.error(`took ${ms.toFixed(1)} ms`);
      client.end();
      return;
    }
    ping();
  }
});
