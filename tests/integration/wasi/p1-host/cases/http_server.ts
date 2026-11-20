// A node:http server answering two requests, then exiting.
import * as http from "node:http";

let served = 0;
const server = http.createServer((req, res) => {
  let body = "";
  req.on("data", (chunk) => (body += chunk));
  req.on("end", () => {
    const text = `${req.method} ${req.url} [${body}]`;
    res.writeHead(200, { "Content-Type": "text/plain", "Content-Length": String(text.length) });
    res.end(text, () => {
      if (++served === 2) setTimeout(() => process.exit(0), 50);
    });
  });
});
server.listen(0, "127.0.0.1", () => console.log("listening"));
