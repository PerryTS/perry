// #4962 e2e: listen(0) shared ephemeral port + SCHED_RR fd-passing distribution.
//
// Primary forks N workers, each `http.createServer(...).listen(0)`. We assert
// (1) every worker reports the SAME server.address().port (primary-coordinated
// shared ephemeral) and (2) under SCHED_RR a batch of sequential connections is
// distributed round-robin (each worker serves an equal share). Each worker
// stamps its cluster id into the response body, so the primary tallies which
// worker served each request. `PERRY_TEST_SCHED=none` selects SCHED_NONE
// (kernel SO_REUSEPORT balancing) to contrast the two mechanisms.
import cluster from "node:cluster";
import http from "node:http";

const NUM_WORKERS = 3;
const TOTAL_REQS = 12;

if (process.env.PERRY_TEST_SCHED === "none") {
  // @ts-ignore — assignable per Node (CJS export)
  cluster.schedulingPolicy = cluster.SCHED_NONE;
}

if (cluster.isPrimary) {
  const ports = new Set<number>();
  const hits: Record<string, number> = {};
  let listeningCount = 0;

  const finish = () => {
    setTimeout(() => {
      const ids = Object.keys(hits).sort();
      console.log("WORKERS_HIT:" + ids.length);
      console.log("DISTRIBUTION:" + JSON.stringify(hits));
      const counts = ids.map((id) => hits[id]);
      const even =
        counts.length === NUM_WORKERS && counts.every((c) => c === TOTAL_REQS / NUM_WORKERS);
      console.log("ROUND_ROBIN_EVEN:" + even);
      for (const id in cluster.workers) cluster.workers[id]!.kill();
      process.exit(0);
    }, 300);
  };

  // RR distributes per CONNECTION, so each request must be its own TCP
  // connection. `Connection: close` + agent:false makes the server close after
  // the response and the client not reuse the socket, so firing sequentially
  // yields one fresh connection per request, accepted one-at-a-time → the
  // primary's accept loop hands them to workers in deterministic rotation.
  const runSequential = (port: number, done: number) => {
    if (done >= TOTAL_REQS) return finish();
    const req = http.get(
      { host: "127.0.0.1", port, agent: false, headers: { Connection: "close" } },
      (res: any) => {
        let body = "";
        res.on("data", (c: any) => (body += c));
        res.on("end", () => {
          const id = body.trim();
          if (id) hits[id] = (hits[id] || 0) + 1;
          runSequential(port, done + 1);
        });
      },
    );
    req.on("error", (e: any) => {
      console.log("REQ_ERR:" + e.message);
      runSequential(port, done + 1);
    });
  };

  cluster.on("listening", (_worker: any, address: any) => {
    ports.add(address.port);
    if (++listeningCount === NUM_WORKERS) {
      console.log("PORTS_SHARED:" + (ports.size === 1));
      console.log("PORT_COUNT:" + ports.size);
      runSequential([...ports][0], 0);
    }
  });

  for (let i = 0; i < NUM_WORKERS; i++) cluster.fork();
} else {
  const id = String(cluster.worker?.id);
  const server = http.createServer((_req: any, res: any) => {
    res.writeHead(200, { "Content-Type": "text/plain" });
    res.end(id);
  });
  server.listen(0);
}
