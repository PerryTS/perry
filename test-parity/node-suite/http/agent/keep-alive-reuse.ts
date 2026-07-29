import { Agent, createServer, get } from "node:http";

const server = createServer((_req, res) => res.end("ok"));
await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});

const agent = new Agent({ keepAlive: true, maxSockets: 1, maxFreeSockets: 1 });
try {
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("missing address");
  }
  const reused: boolean[] = [];
  for (let index = 0; index < 2; index++) {
    await new Promise<void>((resolve, reject) => {
      const req = get(
        { hostname: "127.0.0.1", port: address.port, agent },
        (res) => {
          reused.push(req.reusedSocket);
          res.resume();
          res.on("end", resolve);
        },
      );
      req.once("error", reject);
    });
  }
  const key = agent.getName({ host: "127.0.0.1", port: address.port });
  console.log("reused:", reused.join("|"));
  console.log(
    "pools:",
    agent.sockets[key]?.length ?? 0,
    agent.freeSockets[key]?.length ?? 0,
    agent.requests[key]?.length ?? 0,
  );
  console.log("total:", agent.totalSocketCount);
} finally {
  agent.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
