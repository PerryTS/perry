// HWS hook: js_stdlib_install_external_net_pump
// HWS prefixes: perry_runtime::turnloop_net::|turnloop_net::
// HWS companion: turnloop_net
import { createServer, connect } from "node:net";
const server = createServer(socket => {
  socket.on("data", data => socket.end("pong:" + data.toString()));
});
server.listen(0, "127.0.0.1", () => {
  const client = connect(server.address().port, "127.0.0.1", () => client.write("ping"));
  client.on("data", data => console.log(data.toString()));
  client.on("end", () => server.close(() => console.log("closed")));
});
