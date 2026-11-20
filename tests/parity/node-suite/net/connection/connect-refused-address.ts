// A refused connect names the address it tried, in the message and as
// `err.address` / `err.port` (#11043: mongodb reports
// `connect ECONNREFUSED 127.0.0.1:27017` from this message). The port is
// ephemeral, so it is printed as a placeholder after checking it.
import * as net from "node:net";

const server = net.createServer();
await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const port = (server.address() as net.AddressInfo).port;
await new Promise<void>((resolve) => server.close(() => resolve()));

for (const options of [{ port, host: "127.0.0.1" }, { port, host: "127.0.0.1", family: 4 }]) {
  const error: any = await new Promise((resolve) => {
    net.createConnection(options).once("error", resolve);
  });
  console.log(error.message.replace(`:${port}`, ":<port>"));
  console.log(error.code, error.syscall, error.address, error.port === port);
}
