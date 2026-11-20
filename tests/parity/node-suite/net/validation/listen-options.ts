import * as net from "node:net";

const cases: [string, any][] = [
  ["missing endpoint", {}],
  ["missing endpoint fields", { exclusive: true }],
  ["signal", { port: 0, signal: "bad" }],
];

async function listenAndClose(server: net.Server, options: any): Promise<void> {
  await new Promise<void>((resolve) => {
    server.once("error", () => resolve());
    server.once("listening", () => resolve());
    server.listen(options);
  });
  await new Promise<void>((resolve) => server.close(() => resolve()));
}

for (const [label, options] of cases) {
  const server = net.createServer();
  try {
    await listenAndClose(server, options);
    console.log(label, "OK");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}
