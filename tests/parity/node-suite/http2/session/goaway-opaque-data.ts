import { Buffer } from "node:buffer";
import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  const received = new Promise<void>((resolve) => {
    server.on("session", (session: any) => {
      session.on(
        "goaway",
        (code: number, lastStreamID: number, data: Buffer) => {
          console.log(code, lastStreamID, data.toString());
          resolve();
        },
      );
    });
  });
  server.on("stream", (stream: any) => {
    stream.respond({ ":status": 204 });
    stream.end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  await new Promise<void>((resolve, reject) => {
    const request = client.request();
    request.on("error", reject);
    request.on("end", resolve);
    request.resume();
    request.end();
  });
  client.goaway(0, 0, Buffer.from("bye"));
  await received;
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
