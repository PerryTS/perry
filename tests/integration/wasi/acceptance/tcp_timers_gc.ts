import * as net from "node:net";
import { collect } from "perry/gc";

let ticks = 0;
const canceled = setTimeout(() => console.log("canceled timer fired"), 1);
clearTimeout(canceled);
const server = net.createServer(socket => {
  const retained = { text: "pong" };
  const interval = setInterval(() => {
    collect();
    ticks++;
    if (ticks === 2) {
      clearInterval(interval);
      socket.write(retained.text.slice(0, 2));
      socket.end(retained.text.slice(2));
    }
  }, 5);
});
server.listen(0, "127.0.0.1", () => {
  const client = net.createConnection(server.address().port, "127.0.0.1");
  let text = "";
  client.on("data", data => { collect(); text += data.toString(); });
  client.on("end", () => { console.log(text, ticks); server.close(); });
});
