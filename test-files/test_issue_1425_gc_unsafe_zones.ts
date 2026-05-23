// Regression for issue #1425: a long-running Fastify + ws server must not
// keep the runtime in a process-lifetime GC unsafe zone. Manual gc() calls
// should run while both servers are listening.

declare function gc(): void;

import fastify from "fastify";
import { WebSocket, WebSocketServer } from "ws";

const fastifyPort = parseInt(process.argv[2] || "18951", 10);
const wsPort = parseInt(process.argv[3] || "18952", 10);

const app = fastify();
let wss: WebSocketServer | undefined;

app.get("/ping", async (_request, _reply) => {
  return { ok: true };
});

function churn(round: number): number {
  const keep: any[] = [];
  let total = 0;
  for (let i = 0; i < 1500; i++) {
    const value = {
      id: round * 1500 + i,
      label: "issue1425-" + round + "-" + i,
      nested: { a: i, b: i * 3, c: "payload-" + i },
    };
    keep.push(value);
    total += value.id + value.nested.b;
  }
  return total + keep.length;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main(): Promise<void> {
  await app.listen({ port: fastifyPort, host: "127.0.0.1" });

  wss = new WebSocketServer({ port: wsPort });
  wss.on("connection", (socket: WebSocket) => {
    socket.on("message", (data: any) => {
      socket.send("echo:" + data.toString());
    });
  });

  console.log(
    "issue1425:ready fastify=" + fastifyPort + " ws=" + wsPort,
  );

  let checksum = 0;
  for (let round = 0; round < 4; round++) {
    await sleep(250);
    checksum += churn(round);
    gc();
  }

  console.log("issue1425:manual-gc-done checksum=" + checksum);
  await app.close();
  wss.close();
}

main().catch(async (err: any) => {
  console.log("issue1425:error=" + (err && err.message ? err.message : err));
  await app.close();
  wss?.close();
});
