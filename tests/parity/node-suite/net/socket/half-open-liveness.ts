import net from "node:net";

const reached = setTimeout(() => {
  console.log("still alive");
  process.exit(3);
}, 15000);
reached.unref();

let abandon = () => {};
const half = (port: number) =>
  new Promise<string>((resolve) => {
    let received = "";
    const client = net.connect({ port, host: "127.0.0.1", allowHalfOpen: true }, () => client.end("x"));
    client.on("data", (chunk) => (received += chunk));
    client.on("error", () => {});
    client.on("close", () => resolve(received.length > 100 ? String(received.length) : received));
    abandon = () => client.destroy();
  });

const listen = (onSocket: (socket: net.Socket) => void, run: (port: number) => Promise<void>) =>
  new Promise<void>((resolve) => {
    const server = net.createServer({ allowHalfOpen: true }, (socket) => {
      socket.on("error", () => {});
      socket.resume();
      onSocket(socket);
    });
    server.listen(0, "127.0.0.1", async () => {
      await run((server.address() as net.AddressInfo).port);
      server.close(() => console.log("server close"));
      resolve();
    });
  });

await listen(
  (socket) => socket.on("end", () => setTimeout(() => socket.end("late"), 20)),
  async (port) => console.log("late answer:", await half(port)),
);
const big = "a".repeat(8 * 1024 * 1024);
await listen(
  (socket) => socket.on("end", () => setImmediate(() => socket.end(big))),
  async (port) => console.log("large late answer:", await half(port)),
);
// Only the answer holds the loop: the listener is closed and the peer unreferenced.
await new Promise<void>((resolve) => {
  const server = net.createServer({ allowHalfOpen: true }, (socket) => {
    server.close();
    socket.resume();
    socket.on("end", () => socket.end(big, () => console.log("unaided answer flushed")));
    socket.on("close", () => {
      console.log("unaided socket close");
      resolve();
    });
  });
  server.listen(0, "127.0.0.1", () => {
    const { port } = server.address() as net.AddressInfo;
    const client = net.connect({ port, host: "127.0.0.1", allowHalfOpen: true }, () => client.end("x"));
    client.on("data", () => {});
    client.on("end", () => client.destroy());
    client.unref();
  });
});
// The peer is gone and nothing writes: the half-open socket does not hold the loop.
await listen(
  (socket) => {
    socket.on("end", () => {
      console.log("end");
      abandon();
    });
    socket.on("close", () => console.log("abandoned close"));
  },
  async (port) => console.log("abandoned:", JSON.stringify(await half(port))),
);
process.on("exit", (code) => console.log("exit", code));
