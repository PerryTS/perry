// Census follow-up (gc_runtime_root_holders ffi/ext coverage): perry-ext-net's
// `once_flags()` side table keys once-listener membership by the closure's
// ADDRESS BITS (statics::once_flags(), lib.rs). The canonical copy in
// `listeners()` is scanned — the closure stays alive and its Vec slot is
// REWRITTEN when the copying GC moves it — but a HashSet element cannot be
// rewritten in place, so before scan_net_roots learned to drain/forward/
// reinsert the set, an evacuation between `.once(...)` and the event left
// the set holding the OLD address: the fire-time membership test missed,
// the listener was never auto-removed, and the "once" callback ran again on
// the next event. Two 'data' events are separated CAUSALLY (client acks the
// first before the server sends the second) so GC pauses cannot coalesce
// them, and the once-handler churns the heap so the seeded schedule moves
// the parked closure between registration and each fire. Node (and fixed
// Perry) count 1. Run under the seeded instrument env (see
// test_gap_gc_fetch_method_value_cache_rooting.ts); a green run needs the
// retired_set line and nonzero copying_minors.
import * as net from "node:net";

function churn(): number {
  let keep: Array<{ i: number; s: string }> = [];
  for (let i = 0; i < 30000; i++) {
    keep.push({ i, s: "pad-" + i });
    if (keep.length > 64) keep = [];
  }
  return keep.length;
}

let onceFires = 0;
let dataEvents = 0;

const server = net.createServer((sock) => {
  let responded = false;
  sock.write("first");
  sock.on("data", () => {
    if (responded) return;
    responded = true;
    sock.write("second");
    setTimeout(() => {
      sock.end();
    }, 30);
  });
});

server.listen(0, "127.0.0.1", () => {
  const addr = server.address() as net.AddressInfo;
  const client = net.connect(addr.port, "127.0.0.1");
  client.once("data", () => {
    onceFires++;
    churn();
    client.write("go");
  });
  client.on("data", () => {
    dataEvents++;
  });
  client.on("close", () => {
    console.log("once fired:", onceFires, "of", dataEvents, "events");
    server.close();
  });
  churn();
});
