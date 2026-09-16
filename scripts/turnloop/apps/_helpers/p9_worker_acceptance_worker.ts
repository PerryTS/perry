// The Worker half of `p9_worker_agent_acceptance.ts`.
//
// Three network operations on ONE non-primary agent: an HTTP `fetch`, a raw
// `net.connect`, and one database round-trip. Before P9 all three declined to
// tokio on this thread, because `agent_loop::net_available()` was
// `current_agent() == PRIMARY_AGENT`. After P9 this thread owns a loop of its
// own, so each is expected to take the turnloop path — and the run says which
// it took rather than only whether it worked, because a fallback that happens
// to succeed looks exactly like a migration that did.
//
// Two files because a Worker whose entry is its own module does not link
// (P8's defect 3).
import net from "node:net";
// ioredis: the one driver whose plaintext path P7 migrated and whose server
// needs no schema. REDIS_TLS must be the string "false" or the binding declines
// at construction whatever this lane does (P7 defect 6).
import Redis from "ioredis";
import { parentPort } from "node:worker_threads";

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
    const key = "p9:worker";
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

// Printed as each one finishes, not collected and printed at the end: a throw
// anywhere in the batch would otherwise take every result with it, and "the
// Worker printed nothing" is indistinguishable from "the Worker never ran".
console.log(`worker-agent fetch: ${await doFetch()}`);
console.log(`worker-agent connect: ${await doConnect()}`);
console.log(`worker-agent database: ${await doDatabase()}`);
parentPort?.postMessage("done");
