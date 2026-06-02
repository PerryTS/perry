import * as net from "node:net";

function getConnections(server: net.Server): Promise<[any, number]> {
  return new Promise((resolve) => {
    server.getConnections((err, count) => resolve([err, count]));
  });
}

const server = net.createServer((socket) => {
  console.log("server connection");
  socket.on("error", (err: any) => {
    console.log("accepted error:", err.code || err.message);
  });
});

server.on("drop", (data: any) => {
  console.log("server drop keys:", Object.keys(data).sort().join(","));
  console.log(
    "server drop local:",
    typeof data.localAddress,
    typeof data.localPort,
    data.localFamily,
  );
  console.log(
    "server drop remote:",
    typeof data.remoteAddress,
    typeof data.remotePort,
    data.remoteFamily,
  );
});
server.on("close", () => console.log("server close event"));

console.log(
  "server methods:",
  typeof server.getConnections,
  typeof server.listen,
  typeof server.close,
);
console.log(
  "server initial state:",
  server.listening,
  (server as any).maxConnections,
  (server as any).dropMaxConnection,
);

(server as any).maxConnections = 1;
(server as any).dropMaxConnection = true;
console.log(
  "server assigned state:",
  (server as any).maxConnections,
  (server as any).dropMaxConnection,
);

const [beforeErr, beforeCount] = await getConnections(server);
console.log("getConnections before:", beforeErr && beforeErr.name, beforeCount);

await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
console.log("server listening:", server.listening, typeof server.address()?.port);

const [listeningErr, listeningCount] = await getConnections(server);
console.log("getConnections listening:", listeningErr && listeningErr.name, listeningCount);

const port = (server.address() as any).port;
const client1 = net.connect(port, "127.0.0.1");
await new Promise<void>((resolve) => client1.once("connect", resolve));
console.log("client1 connected");

const [oneErr, oneCount] = await getConnections(server);
console.log("getConnections one:", oneErr && oneErr.name, oneCount);

const client2 = net.connect(port, "127.0.0.1");
client2.on("connect", () => console.log("client2 connected"));
client2.on("error", (err: any) => console.log("client2 error:", err.name, err.code));
client2.on("close", (hadError) => console.log("client2 close:", hadError));

await new Promise((resolve) => setTimeout(resolve, 300));
const [afterErr, afterCount] = await getConnections(server);
console.log("getConnections after second:", afterErr && afterErr.name, afterCount);

client1.destroy();
client2.destroy();
await new Promise<void>((resolve) => server.close(resolve));
console.log("server final state:", server.listening);
