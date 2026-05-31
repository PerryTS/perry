// #2549 — net.Socket initial property shape.
// #2963 — crypto Sign/Verify handle invalidation after a terminal call.
import net from "node:net";
import crypto from "node:crypto";

// ── #2549: a freshly constructed net.Socket exposes Node's initial props ──
const sock = new net.Socket();
console.log("pending", sock.pending);
console.log("connecting", sock.connecting);
console.log("destroyed", sock.destroyed);
console.log("readyState", sock.readyState);
console.log("bytesRead", sock.bytesRead);
console.log("bytesWritten", sock.bytesWritten);
console.log("timeout", sock.timeout);
console.log("localAddress", sock.localAddress);
console.log("localPort", sock.localPort);
console.log("remoteAddress", sock.remoteAddress);
console.log("remotePort", sock.remotePort);

// ── #2963: a Sign handle is consumed by .sign(); reuse throws ──
const privateKey =
  "-----BEGIN PRIVATE KEY-----\n" +
  "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgzXENgrYISpXh8UGG\n" +
  "n6gRdTvn03fTF16DgDQTADm0XV6hRANCAAS86N32ME7tVKj5oIMLOiYoElFNSXbJ\n" +
  "wMQL3GyWDLKC996gWUP4WfQLYOJd6To9wdlomuiOFtVryzwKdMdFFd7G\n" +
  "-----END PRIVATE KEY-----\n";

const s = crypto.createSign("sha256");
s.update("abc");
const sig = s.sign(privateKey);
console.log("sign first ok", sig.length > 0);

try {
  s.sign(privateKey);
  console.log("sign second ok");
} catch (e: any) {
  console.log("sign second throw", e.code, e.message);
}

try {
  s.update("x");
  console.log("update after ok");
} catch (e: any) {
  console.log("update after throw", e.code, e.message);
}
