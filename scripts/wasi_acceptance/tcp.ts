import * as net from "node:net";
const server = net.createServer(socket => socket.end("pong"));
server.listen(0, "127.0.0.1", () => {
  const client = net.createConnection(server.address().port, "127.0.0.1");
  let text = "";
  client.on("data", data => { text += data.toString(); });
  client.on("end", () => { console.log(text); server.close(); });
});
