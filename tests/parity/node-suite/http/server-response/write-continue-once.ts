import * as http from "node:http";
import * as net from "node:net";

const server = http.createServer((_request, response) => {
  response.writeContinue();
  response.end("ok");
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
try {
  const wire = await new Promise<string>((resolve, reject) => {
    const socket = net.connect((server.address() as any).port, "127.0.0.1");
    let data = "";
    socket.setEncoding("utf8");
    socket.on("connect", () => socket.write("GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"));
    socket.on("data", (chunk: string) => { data += chunk; });
    socket.on("end", () => resolve(data));
    socket.on("error", reject);
  });
  console.log((wire.match(/HTTP\/1\.1 100 Continue/g) || []).length);
  console.log(wire.endsWith("ok"));
} finally {
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
