import * as http2 from "node:http2";

function runWarmup(): Promise<void> {
  return new Promise((resolve) => {
    const server = http2.createServer();
    server.on("stream", (stream: any) => {
      stream.respond({ ":status": 204 });
      stream.end();
    });
    server.listen(0, "127.0.0.1", () => {
      const client = http2.connect(`http://127.0.0.1:${server.address().port}`);
      client.on("connect", () => {
        const req = client.request({ ":path": "/warmup", ":method": "GET" });
        req.resume();
        req.on("end", () => {
          client.close(() => {
            server.close(() => resolve());
          });
        });
        req.end();
      });
    });
  });
}

function runProbe(): Promise<void> {
  return new Promise((resolve) => {
    const server = http2.createServer();
    const order: string[] = [];
    let closed = false;

    function closeBoth(client: any) {
      if (closed) {
        return;
      }
      closed = true;
      client.close(() => {
        server.close(() => resolve());
      });
    }

    server.on("session", (session: any) => {
      order.push("server");
      session.on("remoteSettings", () => {});
    });
    server.on("stream", (stream: any) => {
      stream.respond({ ":status": 200 });
      stream.end("ok");
    });
    server.listen(0, "127.0.0.1", () => {
      const client = http2.connect(`http://127.0.0.1:${server.address().port}`);
      client.on("connect", () => {
        order.push("client");
        console.log("probe order:", order.join(">"));
        client.settings({ initialWindowSize: 65535 }, (err: any, settings: any) => {
          console.log("probe settings cb:", err === null, settings.initialWindowSize);
          const req = client.request({ ":path": "/probe", ":method": "GET" });
          req.resume();
          req.on("end", () => closeBoth(client));
          req.end();
        });
      });
    });
  });
}

await runWarmup();
await runProbe();
