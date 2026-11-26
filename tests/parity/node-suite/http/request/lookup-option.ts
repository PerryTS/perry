// `http.request` dials through `options.lookup`, which the agent passes to
// `net.createConnection` with the request's dns options, and the request's
// socket reports the answer through 'lookup' events.
import * as http from "node:http";
import type { AddressInfo } from "node:net";

const server = http.createServer((req, res) => res.end(`host ${req.headers.host}`));
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as AddressInfo).port;

for (const [label, extra] of [
  ["default", {}],
  ["family4", { family: 4 }],
  ["agentless", { agent: false }],
] as const) {
  await new Promise<void>((resolve) => {
    const req = http.request(
      {
        host: "http-lookup.test",
        port,
        ...extra,
        lookup(host: string, options: any, callback: any) {
          console.log(label, "lookup", host, options.family, options.all);
          setTimeout(() => {
            callback(null, options.all ? [{ address: "127.0.0.1", family: 4 }] : "127.0.0.1", 4);
          }, 1);
        },
      },
      (res) => {
        let body = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => (body += chunk));
        res.on("end", () => {
          console.log(label, res.statusCode, body.replace(`:${port}`, ":<port>"));
          resolve();
        });
      },
    );
    req.on("socket", (socket) => {
      socket.on("lookup", (err: any, address: any, family: any, host: any) => {
        console.log(label, "lookup event", err, address, family, host);
      });
    });
    req.on("error", (err: any) => {
      console.log(label, "error", err.code);
      resolve();
    });
    req.end();
  });
}

server.close();
