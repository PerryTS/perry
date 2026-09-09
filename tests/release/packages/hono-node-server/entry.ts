// Issue #8749: @hono/node-server selects its imported node:http factory
// through `options.createServer || createServerHTTP` inside compiled package
// code. Exercise the real package through listen, fetch, response, and close.
import { serve } from "@hono/node-server";
import { Hono } from "hono";

const port = 38139;
const app = new Hono();
app.get("/", (context) => context.text("ok"));
const server = serve({
  fetch: app.fetch,
  port,
}, async () => {
  const response = await fetch(`http://127.0.0.1:${port}/`);
  console.log(`status=${response.status}`);
  console.log(`body=${await response.text()}`);
  server.close();
  process.exit(0);
});
