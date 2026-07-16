import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
for (
  const [label, value] of [
    ["context", tls.createSecureContext({ key, cert })],
    ["options", { key, cert }],
  ] as const
) {
  const socket = new tls.TLSSocket();
  try {
    console.log(label + ":", socket.setKeyCert(value) === undefined);
  } catch (err: any) {
    console.log(label + ":", false, err.code ?? err.name);
  } finally {
    try {
      socket.destroy();
    } catch {}
  }
}
for (const [label, value] of [["null", null], ["number", 1]] as const) {
  const socket = new tls.TLSSocket();
  try {
    socket.setKeyCert(value as any);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  } finally {
    try {
      socket.destroy();
    } catch {}
  }
}
