import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const events: string[] = [];
const server = tls.createServer({ key, cert }, (socket) => {
  events.push("server:secureConnection");
  socket.on("data", (chunk) => {
    events.push("server:data:" + chunk.toString());
    socket.end("pong");
  });
});
server.on("error", (err: any) => events.push("server:error:" + err.code));
server.listen(0, "127.0.0.1", () => {
  events.push("server:listening");
  const address: any = server.address();
  const client = tls.connect({ host: "127.0.0.1", port: address.port, rejectUnauthorized: false });
  client.on("secureConnect", () => {
    events.push("client:secureConnect");
    client.write("ping");
  });
  client.on("data", (chunk) => events.push("client:data:" + chunk.toString()));
  client.on("error", (err: any) => events.push("client:error:" + err.code));
  client.on("close", () => server.close(() => {
    events.push("server:close");
    console.log(events.join("|"));
  }));
});
