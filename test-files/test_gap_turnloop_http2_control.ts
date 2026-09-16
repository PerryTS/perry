// SETTINGS, PING and GOAWAY as frames on the wire.
//
// Perry's HTTP/2 control surface used to be a loopback simulation: `settings()`,
// `goaway()` and `ping()` scanned process handles for a peer session of the
// opposite type and pushed a synthetic event into its queue, so nothing was
// ever encoded and a Perry client could only ever "talk to" a Perry server in
// the same process. These assertions are therefore about the peer OBSERVING the
// frame — a PING acknowledgement carrying the payload back, a server seeing the
// client's SETTINGS as its `remoteSettings`, and a `'goaway'` listener on the
// other side of the connection — none of which a loopback could fake for a real
// peer and none of which had a reference behaviour before.
import * as http2 from "node:http2";

const server = http2.createServer((_req: any, res: any) => res.end("ok"));
let client: any;
try {
  // The assertion is that the server observed THE CLIENT'S OWN SETTINGS frame,
  // so it waits for the value the client asks for below rather than for the
  // handshake's SETTINGS — whose contents differ between engines (Node sends an
  // empty SETTINGS frame; see the report's turnloop gap 11).
  const remote = new Promise<string>((resolve) => {
    server.on("session", (session: any) => {
      session.on("remoteSettings", (s: any) => {
        if (s.maxConcurrentStreams === 7) resolve(String(s.maxConcurrentStreams));
      });
    });
  });
  const goaway = new Promise<string>((resolve) => {
    server.on("session", (session: any) => {
      session.on("goaway", (code: number, lastStreamID: number) =>
        resolve(code + "/" + lastStreamID));
    });
  });

  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = (server.address() as any).port;

  client = http2.connect(`http://127.0.0.1:${port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });

  const payload = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]);
  const echoed = await new Promise<string>((resolve, reject) => {
    const ok = client.ping(payload, (err: any, _ms: number, back: Buffer) => {
      if (err) reject(err);
      else resolve(Buffer.from(back).toString("hex"));
    });
    if (!ok) reject(new Error("ping refused"));
  });
  console.log("ping echo " + echoed);

  const acked = await new Promise<string>((resolve, reject) => {
    client.settings({ maxConcurrentStreams: 7 }, (err: any, settings: any) => {
      if (err) reject(err);
      else resolve(String(settings.maxConcurrentStreams));
    });
  });
  console.log("settings ack " + acked);
  console.log("local maxConcurrentStreams " + client.localSettings.maxConcurrentStreams);
  console.log("server saw remoteSettings maxConcurrentStreams " + (await remote));

  client.goaway(0, 0);
  console.log("server saw goaway " + (await goaway));
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
console.log("done");
