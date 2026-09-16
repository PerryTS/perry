// @covers node:http2 control-surface isolation between concurrent sessions (#10327)
//
// ORACLE: Node 26.5.1. This fixture needs no raw socket: it simply runs TWO
// independent http2 servers with one client each in the same process, and
// checks that a control frame sent on one session is delivered to that
// session's peer and NOWHERE else.
//
// It is here because Perry's control surface does not encode frames at all —
// `queue_session_settings` / `queue_session_goaway` (perry-ext-http
// `server/http2_server/controls.rs`) walk the process-wide handle table with
// `iter_handle_ids_of::<Http2SessionHandle>` and push a synthetic event at
// EVERY handle of the opposite session_type that is neither closed nor
// destroyed. For a SERVER caller the server-handle filter is
// `unwrap_or(true)`, i.e. no filter at all, so a server session's `goaway()`
// or `settings()` reaches every client session in the process — including
// clients connected to a completely different server.
//
// Established against Node: cross-talk is zero. Each event lands on exactly
// one session, identified here by the port it is connected to.
import http2 from "node:http2";
import { barrier, waitEvent } from "./_helpers/h2_wire.ts";

function sleep(ms: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, ms));
}

interface Rig {
  name: string;
  server: any;
  serverSession: any;
  client: any;
  port: number;
}

const events: string[] = [];

async function makeRig(name: string): Promise<Rig> {
  const server: any = http2.createServer();
  let serverSession: any = null;
  server.on("session", (s: any) => {
    if (serverSession === null) serverSession = s;
    s.on("goaway", (code: number, last: number) => {
      events.push(name + ".server got goaway code=" + code + " last=" + last);
    });
    s.on("remoteSettings", (st: any) => {
      events.push(name + ".server got remoteSettings mcs=" + st.maxConcurrentStreams);
    });
  });
  server.on("stream", (stream: any) => {
    stream.respond({ ":status": 200 });
    stream.end(name);
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = server.address().port;
  const client: any = http2.connect("http://127.0.0.1:" + port);
  client.on("goaway", (code: number, last: number) => {
    events.push(name + ".client got goaway code=" + code + " last=" + last);
  });
  client.on("remoteSettings", (st: any) => {
    events.push(name + ".client got remoteSettings mcs=" + st.maxConcurrentStreams);
  });
  if (!(await waitEvent(client, "connect", 800))) console.log("!! client never emitted connect");
  await sleep(120);
  // The connect-time remoteSettings exchange is not what this fixture measures.
  events.length = 0;
  return { name: name, server: server, serverSession: serverSession, client: client, port: port };
}

const alpha = await makeRig("alpha");
const bravo = await makeRig("bravo");
await sleep(150);
events.length = 0;

console.log("== alpha.client.settings({ maxConcurrentStreams: 21 }) ==");
alpha.client.settings({ maxConcurrentStreams: 21 });
await sleep(300);
console.log(JSON.stringify(events.sort()));
events.length = 0;

console.log("== bravo.serverSession.settings({ maxConcurrentStreams: 32 }) ==");
bravo.serverSession.settings({ maxConcurrentStreams: 32 });
await sleep(300);
console.log(JSON.stringify(events.sort()));
events.length = 0;

console.log("== alpha.serverSession.goaway(NO_ERROR, 0) ==");
alpha.serverSession.goaway(http2.constants.NGHTTP2_NO_ERROR, 0);
await sleep(300);
console.log(JSON.stringify(events.sort()));
events.length = 0;

console.log("== bravo.client.goaway(NGHTTP2_ENHANCE_YOUR_CALM) ==");
bravo.client.goaway(http2.constants.NGHTTP2_ENHANCE_YOUR_CALM);
await sleep(300);
console.log(JSON.stringify(events.sort()));
events.length = 0;

console.log("alpha.client destroyed:", alpha.client.destroyed, "bravo.client destroyed:", bravo.client.destroyed);

alpha.client.destroy();
bravo.client.destroy();
await sleep(60);
await barrier(new Promise<void>((r) => alpha.server.close(() => r())), 500);
await barrier(new Promise<void>((r) => bravo.server.close(() => r())), 500);
