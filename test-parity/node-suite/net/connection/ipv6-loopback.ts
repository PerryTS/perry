import * as net from "node:net";

let accepted: net.Socket | undefined;
const server = net.createServer((socket) => {
  accepted = socket;
  socket.end("v6");
});
let client: net.Socket | undefined;
let output = "";

try {
  const listening = await new Promise<boolean>((resolve) => {
    server.once("error", () => resolve(false));
    server.listen(0, "::1", () => resolve(true));
  });
  if (!listening) {
    console.log("ipv6 unavailable");
  } else {
    client = net.connect((server.address() as any).port, "::1");
    client.setEncoding("utf8");
    client.on("data", (data) => output += data);
    client.on("error", () => {});
    await new Promise<void>((resolve) => client!.once("close", resolve));
    console.log(
      "ipv6:",
      (server.address() as any).family,
      client.remoteFamily,
      output,
    );
  }
} finally {
  client?.destroy();
  client?.removeAllListeners();
  accepted?.destroy();
  accepted?.removeAllListeners();
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}
