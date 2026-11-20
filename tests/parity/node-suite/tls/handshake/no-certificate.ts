import tls from "node:tls";

let secure = false;
let clientError = false;
let serverError = false;
const server = tls.createServer();
server.on("tlsClientError", (_err, socket) => {
  serverError = true;
  socket.destroy();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
  });
  client.on("secureConnect", () => {
    secure = true;
    client.destroy();
  });
  client.on("error", () => {
    clientError = true;
  });
  client.on("close", () =>
    server.close(() => {
      console.log("secure:", secure);
      console.log("errors:", clientError, serverError);
    }));
});
