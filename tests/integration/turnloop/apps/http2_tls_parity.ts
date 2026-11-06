// P12 acceptance: `http2.connect('https://…')`, run identically on Perry and on
// Node 26.5.1, against a TLS HTTP/2 server this same file starts.
//
// Before this lane, `http2.connect('https://…')` on Perry did not work at all,
// and had not since the surface existed. `parse_authority` returned port **80**
// for every scheme and the fallback opened a CLEARTEXT `tokio::net::TcpStream`,
// so the HTTP/2 preface went to an HTTPS listener and the peer answered
// `received corrupt message of type InvalidContentType` — the failure the h2c
// lane recorded and could not fix, because there was no public TLS **client**
// installer for a turnloop socket to install a session with.
//
// `alpnProtocol` is printed because ALPN is the whole contract here: an HTTP/2
// client may only speak HTTP/2 if the server selected `h2`, and a client that
// could not offer a protocol list could not ask. It is the one field in this
// output that a cleartext-to-port-80 client could never have produced.
//
// Both halves are in one file on purpose: the server side is already on
// turnloop (it scored h2spec 147/147), so an in-process pair exercises the new
// client against a known-good peer without a second process to keep in step.
//
//   TLS_CERT=<leaf.crt> TLS_KEY=<leaf.key> TLS_CA=<ca.crt>
//
// parity-skip: requires TLS key material
import * as http2 from "http2";
import { readFileSync } from "fs";

const cert = readFileSync(process.env.TLS_CERT ?? "/dev/null");
const key = readFileSync(process.env.TLS_KEY ?? "/dev/null");
const ca = readFileSync(process.env.TLS_CA ?? "/dev/null", "utf8");

async function main(): Promise<void> {
  const server = http2.createSecureServer({ cert, key });
  server.on("stream", (stream, headers) => {
    stream.respond({ ":status": 200, "content-type": "text/plain" });
    stream.end(`path=${headers[":path"]}`);
  });

  const port: number = await new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      resolve(typeof address === "object" && address !== null ? address.port : 0);
    });
  });

  const session = http2.connect(`https://localhost:${port}`, { ca });
  await new Promise<void>((resolve, reject) => {
    session.on("connect", () => resolve());
    session.on("error", reject);
  });

  console.log("alpn:", session.alpnProtocol);
  console.log("encrypted:", session.encrypted === true);

  const body = await new Promise<string>((resolve, reject) => {
    const request = session.request({ ":path": "/hello", ":method": "GET" });
    let status = 0;
    let text = "";
    request.on("response", (headers) => {
      status = Number(headers[":status"]);
    });
    request.setEncoding("utf8");
    request.on("data", (chunk: string) => {
      text += chunk;
    });
    request.on("end", () => resolve(`${status} ${text}`));
    request.on("error", reject);
    request.end();
  });
  console.log("response:", body);

  session.close();
  server.close();
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
