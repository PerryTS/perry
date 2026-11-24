import * as http2 from "node:http2";
import * as net from "node:net";

// A refused connect errors the session with the Error net.connect() gets for
// the same failure, not a bare code string, and a request queued on the
// session is cancelled naming that message (PerryTS/perry#15377).
// `instanceof Error` is left out: net's own socket error is not one yet
// (PerryTS/perry#15382), and this error is net's.
const probe = net.createServer();
await new Promise<void>((resolve) => probe.listen(0, "127.0.0.1", resolve));
const port = (probe.address() as any).port;
await new Promise<void>((resolve) => probe.close(() => resolve()));
const hidePort = (text: string) => text.split(String(port)).join("PORT");

const out: string[] = [];
await new Promise<void>((resolve) => {
  const client = http2.connect(`http://127.0.0.1:${port}`);
  client.on("error", (error: any) => {
    out.push("session error");
    out.push(`  message=${hidePort(String(error.message))}`);
    out.push(`  code=${error.code} syscall=${error.syscall} address=${error.address}`);
    out.push(`  port matches=${error.port === port} errno=${typeof error.errno} negative=${error.errno < 0}`);
  });
  client.on("close", () => {
    out.push("session close");
    resolve();
  });
  const request = client.request({ ":path": "/" });
  request.on("error", (error: any) => out.push(`stream error ${hidePort(String(error.message))}`));
});
console.log(out.join("\n"));
