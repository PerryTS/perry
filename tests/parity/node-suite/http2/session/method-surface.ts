import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  const serverSurface = new Promise<string>((resolve) => {
    server.on("session", (session: any) =>
      resolve(
        [
          "ping",
          "settings",
          "goaway",
          "origin",
          "altsvc",
          "setLocalWindowSize",
          "ref",
          "unref",
        ].map((key) => typeof session[key]).join(","),
      ));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  console.log(
    "client:",
    [
      "request",
      "ping",
      "settings",
      "goaway",
      "setLocalWindowSize",
      "ref",
      "unref",
    ]
      .map((key) => typeof client[key]).join(","),
  );
  console.log("server:", await serverSurface);
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
