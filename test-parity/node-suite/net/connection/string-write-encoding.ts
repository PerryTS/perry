import * as net from "node:net";
import * as tls from "node:tls";
import { readFileSync } from "node:fs";

// The oracle supplies every byte/error result through public Socket methods.
// No private decoder, fixed port, DNS lookup, or external TLS peer is involved.
type Part = { data: any; encoding?: any };
async function journey(label: string, parts: Part[], tail: Part,
  connecting = false, corked = false, encrypted = false) {
  const accepted: net.Socket[] = [];
  const errors: string[] = [];
  const results: string[] = [];
  let bytes = "";
  let callbacks = 0;
  let callbackErrors = 0;
  let finishes = 0;
  let closes = 0;
  let client: net.Socket | undefined;
  let raw: net.Socket | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let peerEnd!: () => void;
  const ended = new Promise<void>((resolve) => { peerEnd = resolve; });
  const accept = (socket: net.Socket) => {
    accepted.push(socket);
    socket.on("data", (chunk) => { bytes += chunk.toString("hex"); });
    socket.once("end", () => { peerEnd(); socket.end(); });
    socket.on("error", (error: any) => errors.push("peer:" + error.code));
  };
  const server = encrypted ? tls.createServer({
    key: readFileSync(new URL("../../tls/fixtures/localhost-key.pem", import.meta.url)),
    cert: readFileSync(new URL("../../tls/fixtures/localhost-cert.pem", import.meta.url)),
  }, accept) : net.createServer(accept);
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    const port = (server.address() as any).port;
    if (encrypted) {
      raw = net.connect(port, "127.0.0.1");
      await new Promise<void>((resolve, reject) => {
        raw!.once("connect", resolve);
        raw!.once("error", reject);
      });
      client = tls.connect({ socket: raw, rejectUnauthorized: false });
    } else client = net.connect(port, "127.0.0.1");
    const callback = (error?: any) => {
      callbacks++;
      if (error) callbackErrors++;
    };
    const send = () => {
      if (corked) client!.cork();
      for (const part of parts) {
        const before = client!.bytesWritten;
        try {
          const encoding = part.encoding;
          const below = client!.write(part.data, encoding, callback);
          results.push("write:" + below);
        } catch (error: any) {
          results.push("throw:" + error.name + ":" + error.code + ":" + error.message
            + ":delta=" + (client!.bytesWritten - before));
        }
      }
      if (corked) {
        results.push("cork-bytes:" + client!.bytesWritten);
        client!.uncork();
      }
      try {
        const encoding = tail.encoding;
        results.push("end-self:" + (client!.end(tail.data, encoding, callback) === client));
      } catch (error: any) {
        results.push("end-throw:" + error.name + ":" + error.code + ":" + error.message);
        client!.end(callback);
      }
    };
    client.on("error", (error: any) => errors.push("client:" + error.code));
    client.on("finish", () => { finishes++; });
    client.on("close", () => { closes++; });
    client.resume();
    const closed = new Promise<void>((resolve) => client!.once("close", resolve));
    const bounded = new Promise<void>((_, reject) => {
      timer = setTimeout(() => reject(new Error(label + " timed out")), 5000);
    });
    if (connecting) send();
    else client.once(encrypted ? "secureConnect" : "connect", send);
    await Promise.race([Promise.all([closed, ended]), bounded]);
    console.log(label, bytes, client.bytesWritten, callbacks, callbackErrors,
      finishes, closes, results.join("|"), errors.join(","));
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    client?.destroy();
    raw?.destroy();
    for (const socket of accepted) socket.destroy();
    if (server.listening) {
      await new Promise<void>((resolve) => server.close(() => resolve()));
    }
    client?.removeAllListeners();
    raw?.removeAllListeners();
    for (const socket of accepted) socket.removeAllListeners();
    server.removeAllListeners();
  }
}

for (const connecting of [false, true]) {
  await journey("hex-" + connecting,
    [{ data: "78", encoding: "hex" }, { data: "68656c6c6f21", encoding: "HeX" }],
    { data: "79", encoding: "hex" }, connecting);
}
const families = [
  { data: "é", encoding: "utf8" }, { data: "hello é", encoding: "UTF-8" },
  { data: "eA==", encoding: "base64" }, { data: "aGVsbG8h", encoding: "BASE64" },
  { data: "-_8", encoding: "base64url" },
  { data: "é", encoding: "latin1" }, { data: "héllo!", encoding: "binary" },
  { data: "é", encoding: "ascii" },
  { data: "x", encoding: "utf16le" }, { data: "hello!", encoding: "UTF-16LE" },
  { data: "y", encoding: "ucs2" }, { data: "hello!", encoding: "ucs-2" },
];
await journey("families", families, { data: "z", encoding: "utf-8" });
await journey("malformed", [
  { data: "787", encoding: "hex" }, { data: "79zz80", encoding: "hex" },
  { data: "zz", encoding: "hex" }, { data: "e A==\n", encoding: "base64" },
  { data: "eA$=", encoding: "base64" }, { data: "a", encoding: "base64" },
], { data: "eQ", encoding: "base64url" });
await journey("defaults", [
  { data: "é" }, { data: "é", encoding: null }, { data: "é", encoding: "" },
  { data: "é", encoding: false }, { data: "é", encoding: 0 },
], { data: "heap-string-é" });
await journey("invalid", [
  { data: "78", encoding: "not-an-encoding" },
  { data: "", encoding: "not-an-encoding" },
  { data: "78", encoding: 42 }, { data: "78", encoding: true },
  { data: "78", encoding: {} },
], { data: "78", encoding: "not-an-encoding" });
await journey("binary-ignores-encoding", [
  { data: Buffer.from([65, 0, 255]), encoding: "hex" },
  { data: new Uint8Array([66, 0, 254]), encoding: "base64" },
  { data: Buffer.from([0, 120, 255]), encoding: "not-an-encoding" },
  { data: new Uint8Array([121, 0, 254]), encoding: 42 },
], { data: Buffer.from([122]), encoding: "hex" });
await journey("cork-decoded", [
  { data: "78", encoding: "hex" }, { data: "aGVsbG8h", encoding: "base64" },
], { data: "79", encoding: "hex" }, false, true);
await journey("tls-encoded", [
  { data: "78", encoding: "hex" }, { data: "68656c6c6f21", encoding: "hex" },
], { data: "eQ==", encoding: "base64" }, false, false, true);

// Validation precedes unopened/destroyed/ended transport behavior. No error
// listener or write callback is fabricated to simulate an encoding failure.
for (const destroyed of [false, true]) {
  const socket = new net.Socket();
  let callbacks = 0;
  let errors = 0;
  socket.on("error", () => { errors++; });
  if (destroyed) socket.destroy();
  try {
    socket.write("78", "not-an-encoding" as any, () => { callbacks++; });
    console.log("invalid-state", destroyed, "returned");
  } catch (error: any) {
    console.log("invalid-state", destroyed, error.name, error.code, error.message,
      socket.bytesWritten, callbacks, errors);
  } finally { socket.destroy(); }
}
