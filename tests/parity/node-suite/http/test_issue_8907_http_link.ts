// #8907: v0.5.1220 on macOS arm64 failed to link this node:http lifecycle
// with 17 undefined HTTP symbols. Keep the listener/close callbacks live.
import { createServer } from "node:http";

function main(): void {
  const server = createServer((_req, res) => { res.end("ok"); });
  server.listen(0, () => {
    console.log("listening");
    server.close(() => { console.log("closed"); });
  });
}

main();
