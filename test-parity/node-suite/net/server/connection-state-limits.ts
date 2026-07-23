import * as net from "node:net";

function getConnections(server: net.Server): Promise<[any, number]> {
  return new Promise((resolve) => {
    server.getConnections((err, count) => resolve([err, count]));
  });
}

let serverConnectionCount = 0;
let acceptedError: any;
let dropData: any;

// The relative order of the independent client-side "connect" and
// server-side "drop" callbacks varies across Node runs. Buffer those
// observations so this fixture measures connection state, not scheduler luck.
const server = net.createServer((socket) => {
  serverConnectionCount++;
  socket.on("error", (err: any) => {
    acceptedError = err;
  });
});

server.on("drop", (data: any) => {
  dropData = data;
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
let client2Connected = false;
let client2Error: any;
let client2Close: boolean | undefined;
client2.on("connect", () => {
  client2Connected = true;
});
client2.on("error", (err: any) => {
  client2Error = err;
});
client2.on("close", (hadError) => {
  client2Close = hadError;
});

await new Promise((resolve) => setTimeout(resolve, 300));
for (let i = 0; i < serverConnectionCount; i++) {
  console.log("server connection");
}
if (acceptedError) {
  console.log("accepted error:", acceptedError.code || acceptedError.message);
}
console.log("server drop keys:", Object.keys(dropData).sort().join(","));
console.log(
  "server drop local:",
  typeof dropData.localAddress,
  typeof dropData.localPort,
  dropData.localFamily,
);
console.log(
  "server drop remote:",
  typeof dropData.remoteAddress,
  typeof dropData.remotePort,
  dropData.remoteFamily,
);
if (client2Connected) {
  console.log("client2 connected");
}
if (client2Error) {
  console.log("client2 error:", client2Error.name, client2Error.code);
}
console.log("client2 close:", client2Close);
const [afterErr, afterCount] = await getConnections(server);
console.log("getConnections after second:", afterErr && afterErr.name, afterCount);

client1.destroy();
client2.destroy();
await new Promise<void>((resolve) => server.close(resolve));
console.log("server final state:", server.listening);
