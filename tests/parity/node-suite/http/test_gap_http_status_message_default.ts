import * as http from "node:http";
import * as net from "node:net";

// Each case sets the status fields the way a framework does, then answers.
type Case = { name: string; handle: (res: http.ServerResponse) => void };
const cases: Case[] = [
  { name: "plain end", handle: (res) => res.end("x") },
  { name: "empty statusMessage end", handle: (res) => { res.statusCode = 200; res.statusMessage = ""; res.end("x"); } },
  { name: "empty statusMessage write", handle: (res) => { res.statusCode = 201; res.statusMessage = ""; res.write("x"); res.end(); } },
  { name: "empty statusMessage flushHeaders", handle: (res) => { res.statusCode = 200; res.statusMessage = ""; res.flushHeaders(); res.end("x"); } },
  { name: "custom statusMessage flushHeaders", handle: (res) => { res.statusCode = 200; res.statusMessage = "Fine"; res.flushHeaders(); res.end("x"); } },
  { name: "writeHead empty reason", handle: (res) => { res.writeHead(404, ""); res.end("x"); } },
];

let current = 0;
const server = http.createServer((req, res) => {
  const i = Number((req.url || "/0").slice(1));
  cases[i].handle(res);
  console.log(cases[i].name + " server statusMessage=" + JSON.stringify(res.statusMessage));
});

function rawStatusLine(port: number, i: number): Promise<string> {
  return new Promise((resolve) => {
    const s = net.connect(port, "127.0.0.1", () => {
      s.write("GET /" + i + " HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    });
    let buf = "";
    s.on("data", (d) => { buf += d.toString(); });
    s.on("end", () => resolve(buf.split("\r\n")[0]));
  });
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as net.AddressInfo).port;
  for (current = 0; current < cases.length; current++) {
    const line = await rawStatusLine(port, current);
    console.log(cases[current].name + " status line=" + JSON.stringify(line));
  }
  server.close();
});
