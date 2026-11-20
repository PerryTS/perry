import { createServer, connect } from "node:http2";

const server = createServer();
let accepted = 0;
let closed = 0;
const events: Record<number, string[]> = {};
server.on("connection", (socket) => {
  const id = ++accepted;
  events[id] = ["connection"];
  socket.on("close", () => {
    closed++;
    events[id].push("close");
  });
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as any).port;

for (const mode of ["graceful", "destroy"]) {
  const client = connect(`http://127.0.0.1:${port}`);
  await new Promise<void>((resolve) => client.on("connect", resolve));
  if (mode === "graceful") {
    await new Promise<void>((resolve) => client.close(resolve));
  } else {
    client.destroy();
  }
}
await new Promise<void>((resolve) => setTimeout(resolve, 250));
await new Promise<void>((resolve) => server.close(resolve));
await new Promise<void>((resolve) => setTimeout(resolve, 250));
console.log("accepted", accepted, "closed", closed);
for (let id = 1; id <= accepted; id++) {
  console.log("socket", id, events[id].join(" > "));
}
