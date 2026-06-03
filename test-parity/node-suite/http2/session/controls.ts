import * as http2 from "node:http2";

const server = http2.createServer();
let closed = false;
let watchSettings = false;
let activeClient: any;
let guard: any;

function closeBoth(client: any) {
  if (closed || !client) {
    return;
  }
  closed = true;
  client.close();
  server.close();
}

server.on("session", (session: any) => {
  console.log(
    "server controls:",
    typeof session.ping,
    typeof session.settings,
    typeof session.goaway,
    typeof session.setLocalWindowSize,
    typeof session.setTimeout,
    typeof session.ref,
    typeof session.unref,
  );
  console.log(
    "server settings shape:",
    session.type,
    session.localSettings.headerTableSize,
    session.remoteSettings.initialWindowSize,
    typeof session.state.localWindowSize,
    typeof session.socket,
  );
  session.on("remoteSettings", (settings: any) => {
    if (watchSettings) {
      console.log("server remoteSettings event:", settings.initialWindowSize, settings.enablePush);
    }
  });
  session.on("goaway", (code: number, lastStreamID: number, opaqueData: Buffer) => {
    console.log(
      "server goaway event:",
      code,
      lastStreamID,
      Buffer.isBuffer(opaqueData),
      opaqueData.toString("utf8"),
    );
    clearTimeout(guard);
    closeBoth(activeClient);
  });
});

server.on("stream", (stream: any) => {
  stream.respond({ ":status": 200 });
  stream.end("ok");
});

server.listen(0, "127.0.0.1", () => {
  console.log("listen port type:", typeof server.address().port);
  const client = http2.connect(`http://127.0.0.1:${server.address().port}`);
  activeClient = client;
  guard = setTimeout(() => {
    console.log("callback guard");
    closeBoth(client);
  }, 500);

  client.on("connect", () => {
    console.log(
      "client controls:",
      typeof client.ping,
      typeof client.settings,
      typeof client.goaway,
      typeof client.setLocalWindowSize,
      typeof client.setTimeout,
      typeof client.ref,
      typeof client.unref,
    );
    console.log(
      "client settings shape:",
      client.type,
      client.localSettings.headerTableSize,
      client.remoteSettings.initialWindowSize,
      typeof client.state.localWindowSize,
      typeof client.socket,
    );
    console.log(
      "control returns:",
      typeof client.ref(),
      typeof client.unref(),
      typeof client.setLocalWindowSize(131072),
      client.setTimeout(0) === client,
    );

    const pingReturn = client.ping(Buffer.from("abcdefgh"), (err: any, duration: number, payload: Buffer) => {
      console.log("ping cb:", err === null, typeof duration, Buffer.isBuffer(payload), payload.toString("utf8"));
      watchSettings = true;
      const settingsReturn = client.settings({ initialWindowSize: 65535 }, (settingsErr: any, settings: any) => {
        console.log("settings cb:", settingsErr === null, settings.initialWindowSize, settings.enablePush);
        const req = client.request({ ":path": "/controls", ":method": "GET" });
        req.resume();
        req.on("end", () => {
          console.log("request end");
          console.log("goaway return:", typeof client.goaway(0, 0, Buffer.from("bye")));
        });
        req.end();
      });
      console.log("settings return:", typeof settingsReturn);
    });
    console.log("ping return:", pingReturn);
  });

  client.on("error", (err: any) => {
    console.log("client error:", err && (err.code || err.message));
    closeBoth(client);
  });
});
