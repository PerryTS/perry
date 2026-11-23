import http from "node:http";
import net from "node:net";
const server = net.createServer((s) => { s.on("error", () => {}); s.once("data", () => s.write("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nab")); });
server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as any).port;
  const ev: string[] = [];
  const req = http.get({ host: "127.0.0.1", port, path: "/", agent: false }, (res) => {
    ev.push("response");
    res.on("data", () => { ev.push("data"); req.destroy(); });
    res.on("aborted", () => ev.push("res aborted"));
    res.on("error", (e: any) => ev.push("res error " + e.code + " " + e.message));
    res.on("end", () => ev.push("res end"));
    res.on("close", () => { ev.push("res close " + res.destroyed + " " + res.complete); });
  });
  req.on("error", (e: any) => ev.push("req error " + e.code));
  req.on("close", () => ev.push("req close"));
  setTimeout(() => { console.log(ev.join("\n")); server.close(); }, 300);
});
