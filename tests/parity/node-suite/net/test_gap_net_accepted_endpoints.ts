// #10344: accepted sockets travel through an untyped callback parameter.
import * as net from "node:net";
const fresh: any = new net.Socket();
console.log("unconnected", fresh.remoteAddress === undefined, fresh.remotePort === undefined, fresh.localAddress === undefined, fresh.localPort === undefined);
fresh.destroy();
const server = net.createServer((socket: any) => {
  console.log("remote", socket.remoteAddress === "127.0.0.1", socket.remotePort > 0, socket.remoteFamily === "IPv4");
  console.log("local", socket.localAddress === "127.0.0.1", socket.localPort === server.address().port, socket.localFamily === "IPv4");
  socket.end();
  server.close();
});
server.listen(0, "127.0.0.1", () => {
  const client = net.createConnection({ port: server.address().port, host: "127.0.0.1" });
  client.on("end", () => client.end());
});
