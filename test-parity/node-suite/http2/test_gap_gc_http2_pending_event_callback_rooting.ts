// Census follow-up (gc_runtime_root_holders ffi/ext coverage): perry-ext-http
// parks the user callbacks of `session.close(cb)` / `session.settings(obj,
// cb)` / `session.ping(cb)` as raw NaN-box bits inside H2_PENDING_EVENTS
// (server/http2_server.rs) between the JS-side dispatch and the main-thread
// drain. The settings/ping callbacks have NO other holder in that window,
// so before scan_h2_pending_event_roots existed a full collection freed
// them and a copying minor left the queue pointing into from-space — the
// drain then called through a dangling closure. The churn between each
// dispatch and the pump tick is what gives a seeded schedule a window. Run
// under the seeded instrument env (see
// test_gap_gc_fetch_method_value_cache_rooting.ts); a green run needs the
// retired_set line and nonzero copying_minors.
import http2 from "node:http2";

function churn(): number {
  let keep: Array<{ i: number; s: string }> = [];
  for (let i = 0; i < 30000; i++) {
    keep.push({ i, s: "pad-" + i });
    if (keep.length > 64) keep = [];
  }
  return keep.length;
}

const server = http2.createServer();
server.listen(0, () => {
  const port = (server.address() as any).port;
  const client = http2.connect(`http://127.0.0.1:${port}`);
  client.on("error", (e: any) => console.log("client error:", e && e.message));
  client.settings({ enablePush: false }, () => {
    console.log("settings cb fired");
    client.ping(() => {
      console.log("ping cb fired");
      client.close(() => {
        console.log("close cb fired");
        server.close();
      });
    });
    churn();
  });
  churn();
});
