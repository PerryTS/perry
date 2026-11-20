import * as http2 from "node:http2";

async function runWarmup(): Promise<void> {
  const server = http2.createServer();
  let client: any;
  try {
    server.on("session", () => {});
    server.on("stream", (stream: any) => {
      stream.respond({ ":status": 204 });
      stream.end();
    });
    await new Promise<void>((resolve, reject) => {
      server.on("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    client = http2.connect(
      `http://127.0.0.1:${(server.address() as any).port}`,
    );
    await new Promise<void>((resolve, reject) => {
      client.on("error", reject);
      client.on("connect", resolve);
    });
    await new Promise<void>((resolve, reject) => {
      const request = client.request({ ":path": "/warmup" });
      request.on("error", reject);
      request.on("end", resolve);
      request.resume();
      request.end();
    });
    await new Promise<void>((resolve) => client.close(resolve));
  } finally {
    client?.destroy();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}

async function runProbe(): Promise<void> {
  const server = http2.createServer();
  let client: any;
  try {
    server.on("session", () => {});
    server.on("stream", (stream: any) => {
      stream.respond({ ":status": 200 });
      stream.end("ok");
    });
    await new Promise<void>((resolve, reject) => {
      server.on("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    client = http2.connect(
      `http://127.0.0.1:${(server.address() as any).port}`,
    );
    await new Promise<void>((resolve, reject) => {
      client.on("error", reject);
      client.on("connect", () => {
        console.log("probe order: client");
        resolve();
      });
    });
    await new Promise<void>((resolve, reject) => {
      client.settings(
        { initialWindowSize: 65535 },
        (error: any, settings: any) => {
          if (error) return reject(error);
          console.log(
            "probe settings cb:",
            error === null,
            settings.initialWindowSize,
          );
          resolve();
        },
      );
    });
    await new Promise<void>((resolve, reject) => {
      const request = client.request({ ":path": "/probe" });
      request.on("error", reject);
      request.on("end", resolve);
      request.resume();
      request.end();
    });
    await new Promise<void>((resolve) => client.close(resolve));
  } finally {
    client?.destroy();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}

await runWarmup();
await runProbe();
