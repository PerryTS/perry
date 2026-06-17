import * as http2 from "node:http2";

const server = http2.createServer();
let closed = false;
let activeClient: any;
let clientConnected = false;
let serverSessionLine = "";
let clientFlowStarted = false;

function closeBoth(client: any) {
  if (closed) {
    return;
  }
  closed = true;
  client.close(() => console.log("client close cb"));
  server.close(() => console.log("server close cb"));
}

function maybeStartClientFlow() {
  if (clientFlowStarted || !clientConnected || !serverSessionLine) {
    return;
  }
  clientFlowStarted = true;
  const client = activeClient;
  console.log(serverSessionLine);
  console.log("client session:", client.type, client.encrypted, client.connecting);
  console.log(
    "client session props:",
    typeof client.localSettings,
    typeof client.remoteSettings,
    typeof client.state,
    typeof client.socket,
  );
  console.log("client request typeof:", typeof client.request);

  const req = client.request({ ":path": "/probe?x=1", ":method": "GET" });
  console.log(
    "client stream initial:",
    typeof req.id,
    req.pending,
    req.closed,
    req.destroyed,
    typeof req.session,
  );
  console.log("client stream helpers:", typeof req.close, typeof req.setTimeout, typeof req.priority);

  let body = "";
  req.on("response", (headers: any) => {
    console.log("client response:", headers[":status"], headers["x-probe"]);
  });
  req.setEncoding("utf8");
  req.on("data", (chunk: string) => {
    body += chunk;
  });
  req.on("end", () => {
    console.log("client body:", body);
    console.log("client stream end state:", req.closed, req.destroyed);
    closeBoth(client);
  });
  req.end();
}

server.on("session", (session: any) => {
  serverSessionLine = `server session: ${session.type} ${session.encrypted} ${session.alpnProtocol}`;
  maybeStartClientFlow();
});

server.on("request", (req: any, res: any) => {
  console.log("request:", req.method, req.url, req.httpVersion, req.headers[":path"]);
  console.log("response end typeof:", typeof res.end);
});

server.on("stream", (stream: any, headers: any) => {
  console.log(
    "stream:",
    headers[":method"],
    headers[":path"],
    typeof stream.id,
    stream.pending,
    stream.closed,
    stream.destroyed,
  );
  console.log("stream session type:", stream.session.type);
  console.log(
    "stream helpers:",
    typeof stream.respond,
    typeof stream.end,
    typeof stream.close,
    typeof stream.setTimeout,
    typeof stream.priority,
  );
  stream.respond({ ":status": 201, "x-probe": "yes" });
  console.log("stream headersSent/sent:", stream.headersSent, stream.sentHeaders[":status"]);
  stream.end("hello h2");
});

server.listen(0, "127.0.0.1", () => {
  console.log("listen port typeof:", typeof server.address().port);
  const client = http2.connect(`http://127.0.0.1:${server.address().port}`);
  activeClient = client;

  client.on("connect", () => {
    clientConnected = true;
    maybeStartClientFlow();
  });

  client.on("error", (err: any) => {
    console.log("client error:", err && err.code ? err.code : err && err.message);
    closeBoth(client);
  });
});
