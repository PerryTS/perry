import { readFileSync } from "node:fs";
import * as https from "node:https";
import * as tls from "node:tls";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
const agent = new https.Agent();
const server = https.createServer({ key, cert });
server.listen(0, "127.0.0.1", () => {
  let socket: any;
  try {
    socket = agent.createConnection((server.address() as any).port, {
      host: "127.0.0.1",
      rejectUnauthorized: false,
    });
  } catch (error: any) {
    console.log("error:", error.name, error.code);
    agent.destroy();
    server.close();
    return;
  }
  console.log("return:", socket instanceof tls.TLSSocket, socket?.encrypted);
  if (!socket) {
    agent.destroy();
    server.close();
    return;
  }
  socket.once("secureConnect", () => {
    console.log("connected:", socket.authorized, socket.authorizationError);
    socket.destroy();
    agent.destroy();
    server.close();
  });
  socket.once("error", (error: any) => {
    console.log("error:", error.code);
    agent.destroy();
    server.close();
  });
});
