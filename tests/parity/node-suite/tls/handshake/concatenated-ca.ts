import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const unrelated = readFileSync(new URL("api-local-cert.pem", fixture));
const cases: Array<[string, string | Buffer[]]> = [
  ["array", [unrelated, cert]],
  ["string", unrelated.toString() + "\n" + cert.toString()],
];
run(0);

function run(index: number) {
  if (index === cases.length) return;
  const [label, ca] = cases[index];
  const server = tls.createServer({ key, cert }, (socket) => socket.end());
  server.listen(0, "127.0.0.1", () => {
    const client = tls.connect({
      host: "127.0.0.1",
      port: (server.address() as any).port,
      servername: "localhost",
      ca,
    });
    client.on(
      "secureConnect",
      () => console.log(label + ":", client.authorized),
    );
    client.on("error", (err: any) => console.log(label + " error:", err.code));
    client.on("close", () => server.close(() => run(index + 1)));
  });
}
