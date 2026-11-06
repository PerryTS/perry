import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const events: string[] = [];
let clientClosed = false;
let serverSocketClosed = false;
let serverClosed = false;
function report() {
  if (clientClosed && serverSocketClosed && serverClosed) console.log(events.join("|"));
}
const server = tls.createServer({ key, cert }, (socket) => {
  socket.on("end", () => events.push("server:end"));
  socket.on("close", () => { events.push("server:socket-close"); serverSocketClosed = true; report(); });
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false });
  client.on("secureConnect", () => events.push("client:secureConnect"));
  client.on("end", () => events.push("client:end"));
  client.on("close", () => {
    events.push("client:close");
    clientClosed = true;
    server.close(() => {
      events.push("server:close");
      serverClosed = true;
      report();
    });
  });
});
