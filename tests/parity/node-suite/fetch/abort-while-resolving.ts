// A fetch aborted while its host name resolves abandons the connection: the
// lookup's answer opens nothing, so the program exits once the server closes.
import * as net from "node:net";

const server = net.createServer((socket) => socket.destroy());
server.listen(0, "127.0.0.1", async () => {
  const { port } = server.address() as net.AddressInfo;
  const controller = new AbortController();
  const response = fetch(`http://localhost:${port}/`, { signal: controller.signal });
  controller.abort();
  try {
    await response;
  } catch (error: any) {
    console.log("fetch", error.name);
  }
  setTimeout(() => server.close(() => console.log("server closed")), 100);
});
