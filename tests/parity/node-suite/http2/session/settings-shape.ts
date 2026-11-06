import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  console.log(
    "local:",
    client.localSettings.initialWindowSize,
    client.localSettings.maxFrameSize,
  );
  const remote = client.remoteSettings;
  console.log(
    "remote:",
    remote === null
      ? "null"
      : `${remote.initialWindowSize} ${remote.maxFrameSize}`,
  );
  console.log("pending ack:", client.pendingSettingsAck);
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
