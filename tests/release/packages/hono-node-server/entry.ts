// Issue #8749: @hono/node-server selects its imported node:http factory
// through `options.createServer || createServerHTTP` inside compiled package
// code. Exercise the real package through listen, fetch, response, and close.
import { serve } from "@hono/node-server";

const port = 38139;
const server = serve({
  fetch: () => new Response("ok"),
  port,
}, async () => {
  const response = await fetch(`http://127.0.0.1:${port}/`);
  console.log(`status=${response.status}`);
  console.log(`body=${await response.text()}`);
  server.close();
  process.exit(0);
});
