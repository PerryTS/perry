// The two loopback servers `p9_worker_agent_acceptance.ts` and
// `p9_agent_loop_rss.ts` need: an HTTP origin and a TCP echo. Run under Node
// (not Perry) so the subject under test is only ever the client side.
//
//   node scripts/turnloop/apps/_helpers/p9_servers.mjs &
import http from "node:http";
import net from "node:net";

const httpPort = Number(process.env.P9_HTTP_PORT ?? "8099");
const echoPort = Number(process.env.P9_ECHO_PORT ?? "8098");
const body = "p9-origin\n".repeat(16);

http
  .createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/plain", "content-length": String(body.length) });
    res.end(body);
  })
  .listen(httpPort, "127.0.0.1", () => console.log(`http on ${httpPort}`));

net
  .createServer((sock) => {
    sock.on("data", (chunk) => sock.write(chunk));
    sock.on("error", () => {});
  })
  .listen(echoPort, "127.0.0.1", () => console.log(`echo on ${echoPort}`));
