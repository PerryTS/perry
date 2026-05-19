// Issue #1132 — native-instance tags on outer arrow-callback params
// must NOT leak into an inner arrow callback that re-binds the same
// name.
//
// Pre-fix: `lookup_native_instance` was first-match over a push-only
// Vec, and the pre-scan that tags `(req, res)` from
// `http.createServer` (and `(sock)` from `net.createServer`, etc.)
// registered into the enclosing scope BEFORE the handler arrow's
// own scope, so the tag was never truncated. An inner callback that
// re-used the conventional name (`res`, `sock`) resolved to the
// OUTER tag — e.g. the inner http.get `(res)` (an IncomingMessage)
// dispatched through the outer createServer `res`'s ServerResponse
// rows, so `res.on('data')` / `res.on('end')` were silent no-ops.
//
// Fix: last-match-wins `lookup_native_instance` (inner registration
// shadows outer) + pre-scan param tags scoped to the call that owns
// the handler arrow (dropped when the call's lowering returns, so an
// inner callback's tag doesn't leak back into the outer body).
//
// This test exercises three shadowing shapes, run SEQUENTIALLY so
// the output is deterministic for parity comparison:
//
//   1. httpCreateServer((req, res) => …) + inner httpGet(url, (res)
//      => …) — the canonical #1132 case. The inner `res` must route
//      `res.on('data')` / `res.on('end')` through the client-side
//      IncomingMessage path; pre-fix the listeners never registered
//      so 'end' never fired and the body was empty.
//
//   2. netCreateServer((sock) => …) + inner netConnect(…, (sock) =>
//      { sock.write(…) }) — the #1132 "same bug shape probably
//      affects net" note. The inner `sock` is the client Socket;
//      `sock.write("hi")` must reach the server (pre-fix the inner
//      `sock` inherited the outer server-connection socket's tag).
//
//   3. Triple-nest on the name `r`: outer httpCreateServer `(req,
//      r)` (ServerResponse) where `r.end(...)` must terminate the
//      response, and an inner httpGet `(r)` (IncomingMessage). After
//      the inner callback's lowering scope closes, the OUTER `r`
//      must still be the ServerResponse — proven because both the
//      server `r.end("zzz")` AND the client `r.on('end')` complete.
//
// Expected stdout (deterministic):
//
//   READY
//   HTTP body=ping
//   NET got=hi
//   TRIPLE body=zzz
//   DONE

import { createServer as httpCreateServer, get as httpGet } from "node:http";
import { createServer as netCreateServer, connect as netConnect } from "node:net";

const HTTP_PORT = 18981;
const NET_PORT = 18982;
const TRIPLE_PORT = 18983;

// ── Case 3 (run last): triple-nest on `r` ──────────────────────────
function runTriple() {
    const tripleServer = httpCreateServer((req: any, r: any) => {
        // Outer `r` = ServerResponse. The inner httpGet below ALSO
        // binds `r`; after that inner callback's lowering scope
        // closes, this outer `r` must still be the ServerResponse so
        // `r.end(...)` terminates the response.
        r.statusCode = 200;
        r.end("zzz");
    });
    tripleServer.listen(TRIPLE_PORT, () => {
        httpGet("http://127.0.0.1:" + TRIPLE_PORT + "/", (r: any) => {
            // Inner `r` = IncomingMessage. Shadows the outer
            // `(req, r)` ServerResponse tag. Must route client-side.
            const parts: any[] = [];
            r.on("data", (c: any) => {
                parts.push(c);
            });
            r.on("end", () => {
                console.log("TRIPLE body=" + Buffer.concat(parts).toString());
                tripleServer.close();
                console.log("DONE");
            });
        });
    });
}

// ── Case 2: net createServer (sock) + inner netConnect (sock) ──────
function runNet() {
    const netServer = netCreateServer((sock: any) => {
        sock.on("data", (chunk: any) => {
            console.log("NET got=" + chunk.toString());
            sock.end();
        });
    });
    netServer.listen(NET_PORT, () => {
        // Inner `sock` = the client Socket, same name as the outer
        // server-connection handler param. `sock.write("hi")` must
        // hit the client socket's writer — pre-#1132 the inner
        // `sock` inherited the outer server-connection socket's tag.
        const client = netConnect(NET_PORT, "127.0.0.1", () => {
            client.write("hi");
        });
        client.on("close", () => {
            netServer.close();
            runTriple();
        });
    });
}

// ── Case 1: http createServer (req, res) + inner httpGet (res) ─────
const httpServer = httpCreateServer((req: any, res: any) => {
    // Outer `res` = ServerResponse. `res.end(...)` must terminate
    // the response so the client's 'end' fires.
    res.statusCode = 200;
    res.end("ping");
});
httpServer.listen(HTTP_PORT, () => {
    // Inner `res` = IncomingMessage (same name as the outer handler
    // param). Must route through the client-side IncomingMessage
    // dispatch — pre-#1132 it inherited the outer ServerResponse tag.
    httpGet("http://127.0.0.1:" + HTTP_PORT + "/", (res: any) => {
        const chunks: any[] = [];
        res.on("data", (c: any) => {
            chunks.push(c);
        });
        res.on("end", () => {
            console.log("HTTP body=" + Buffer.concat(chunks).toString());
            httpServer.close();
            runNet();
        });
    });
});

console.log("READY");

// Self-terminating safety net.
setTimeout(() => {}, 3000);
