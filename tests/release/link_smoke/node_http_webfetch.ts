// Link smoke (#1652 / #589): a stock `perry compile` must link node:http's
// server FFIs (`js_node_http_create_server` / `js_node_http_res_end` / …)
// together with the Web Fetch `Headers` / `Request` / `Response`
// constructors. Pre-fix this failed at link with
// `Undefined symbols: _js_node_http_create_server` for any program that did
// `import { createServer } from "node:http"`.
//
// Host-only: perry-ext-http (the node:http server staticlib) isn't
// cross-compiled to the mobile targets, so the release sweep runs this on
// the host. Compile + run, but never `.listen()` — the regression this
// guards is in LINKING, and skipping the bind keeps the smoke fast and
// non-flaky.
import { createServer } from "node:http";

const headers = new Headers();
headers.set("content-type", "application/json");

const req = new Request("http://localhost/healthz", { method: "GET", headers });
const res = new Response(JSON.stringify({ ok: true }), { status: 200, headers });

// Constructing the server forces the createServer FFI into the link; the
// dead branch keeps `listen` referenced without actually binding a port.
const server = createServer((_req, _res) => {});
if (req.method === "NEVER") server.listen(0);

console.log("node-http-webfetch ok", res.status, headers.get("content-type"));
