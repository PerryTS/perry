// turnloop P9 acceptance: every JS agent has a loop, so every JS agent's
// network I/O goes through turnloop.
//
// The same three operations — `fetch`, `net.connect`, one database query — run
// twice: once on the primary agent and once inside a `node:worker_threads`
// Worker, which is the surface a Node program actually uses for a second JS
// heap. Before P9 the Worker's three declined to reqwest / a tokio
// `TcpStream` / the `redis` crate, because the submit guard asked "am I the
// primary agent?". They are the four `perry-ext-net` and `perry-ext-http`
// edges of P8's removal-plan group A.
//
// What makes this a measurement rather than a smoke test: run it with
// `PERRY_LOOP_STATS=1` and read the per-agent `[perry-loop] driver=turnloop
// … agent=N` lines on stderr. A Worker line with `turns>0` and
// `completions>0` is the evidence that that agent's own loop carried the
// work; the process-wide `p6 http_submitted=/declined=` line says the fetch
// did not fall back. A green run whose Worker printed no `[perry-loop]` line
// at all proves nothing — the operations would have succeeded on tokio too.
//
// Needs, all on 127.0.0.1: an HTTP origin at $P9_URL (default :8099), a TCP
// echo at $P9_ECHO_PORT (default 8098), and a Redis at $P9_REDIS_PORT
// (default 56379). `scripts/turnloop/apps/_helpers/p9_servers.mjs` starts the
// first two under Node; P7's `dbservers.sh` starts the Redis.
//
// The Redis half additionally needs REDIS_HOST / REDIS_PORT / REDIS_TLS=false
// in the ENVIRONMENT, of the parent and therefore of the Worker it inherits to:
// Perry's ioredis binding ignores the constructor argument and declines outright
// unless REDIS_TLS is the literal string "false".
import net from "node:net";
// ioredis: the one driver whose plaintext path P7 migrated and whose server
// needs no schema. REDIS_TLS must be the string "false" or the binding declines
// at construction whatever this lane does (P7 defect 6).
import Redis from "ioredis";
import { Worker } from "node:worker_threads";

const url = process.env.P9_URL ?? "http://127.0.0.1:8099/";
const echoPort = Number(process.env.P9_ECHO_PORT ?? "8098");
const redisPort = Number(process.env.P9_REDIS_PORT ?? "56379");

async function doFetch(): Promise<string> {
  try {
    const r = await fetch(url);
    const body = await r.text();
    return `status=${r.status} bytes=${body.length}`;
  } catch (e) {
    return "error:" + (e as Error).message;
  }
}

function doConnect(): Promise<string> {
  return new Promise<string>((resolve) => {
    const sock = net.connect(echoPort, "127.0.0.1");
    let seen = "";
    sock.on("connect", () => sock.write("p9\n"));
    sock.on("data", (chunk: unknown) => {
      seen += typeof chunk === "string" ? chunk : String(chunk);
      sock.end();
    });
    sock.on("close", () => resolve(`echo=${JSON.stringify(String(seen).trim())}`));
    sock.on("error", (e: Error) => resolve("error:" + e.message));
  });
}

// `set` / `get` / `del`, not `ping`: `ping` and `echo` exist as `js_ioredis_*`
// symbols but have no row in the compiler's native-method table, so they return
// `undefined` on BOTH transports (P7's own probe says so). Asserting them would
// assert that defect instead of this migration -- and an earlier revision of
// this file did exactly that, printing `ping=undefined` as if it were a pass.
//
// Perry's binding also IGNORES the constructor argument and reads REDIS_HOST /
// REDIS_PORT / REDIS_TLS from the environment; `REDIS_TLS` must be the literal
// string "false" or it declines at construction whatever this lane does (P7
// defect 6, perry#10335). The run sets both, and a mismatch shows up as a
// connection error rather than as a silent tokio fallback.
async function doDatabase(): Promise<string> {
  try {
    const client = new Redis({ port: redisPort, host: "127.0.0.1" });
    const key = "p9:primary";
    await client.del(key);
    const stored = await client.set(key, "p9-value");
    const loaded = await client.get(key);
    const removed = await client.del(key);
    await client.quit();
    return `set=${stored} get=${loaded} del=${removed}`;
  } catch (e) {
    return "error:" + (e as Error).message;
  }
}

console.log(`primary-agent fetch: ${await doFetch()}`);
console.log(`primary-agent connect: ${await doConnect()}`);
console.log(`primary-agent database: ${await doDatabase()}`);

const workerUrl = new URL("./_helpers/p9_worker_acceptance_worker.ts", import.meta.url);
const w = new Worker(workerUrl);
await new Promise<void>((resolve) => {
  w.on("message", () => resolve());
  w.on("error", (e: Error) => {
    console.log("worker error:", e.message);
    resolve();
  });
});
await w.terminate();
console.log("done");
