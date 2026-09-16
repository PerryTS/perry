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
    sock.on("data", (chunk: Buffer) => {
      seen += chunk.toString();
      sock.end();
    });
    sock.on("close", () => resolve(`echo=${JSON.stringify(seen.trim())}`));
    sock.on("error", (e: Error) => resolve("error:" + e.message));
  });
}

async function doDatabase(): Promise<string> {
  try {
    const client = new Redis({ port: redisPort, host: "127.0.0.1" });
    const pong = await client.ping();
    const echoed = await client.echo("p9");
    await client.quit();
    return `ping=${pong} echo=${echoed}`;
  } catch (e) {
    return "error:" + (e as Error).message;
  }
}

const fetched = await doFetch();
const connected = await doConnect();
const queried = await doDatabase();
console.log(`worker-agent fetch: ${fetched}`);
console.log(`worker-agent connect: ${connected}`);
console.log(`worker-agent database: ${queried}`);
parentPort?.postMessage("done");
