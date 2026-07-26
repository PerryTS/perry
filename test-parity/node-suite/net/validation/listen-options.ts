import * as net from "node:net";

const cases: [string, any][] = [
  ["missing endpoint", {}],
  ["missing endpoint fields", { exclusive: true }],
  ["signal", { port: 0, signal: "bad" }],
];

for (const [label, options] of cases) {
  const server = net.createServer();
  try {
    server.listen(options);
    server.close();
    console.log(label, "OK");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}
