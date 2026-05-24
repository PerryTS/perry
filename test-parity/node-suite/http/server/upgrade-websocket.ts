import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { connect } from "node:net";

const PORT = 19022;
const KEY = "dGhlIHNhbXBsZSBub25jZQ==";
const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

function acceptValue(key: string) {
  return createHash("sha1").update(key + GUID).digest("base64");
}

function maskedTextFrame(text: string) {
  const payload = Buffer.from(text);
  const mask = Buffer.from([1, 2, 3, 4]);
  const frame = Buffer.alloc(6 + payload.length);
  frame[0] = 0x81;
  frame[1] = 0x80 | payload.length;
  mask.copy(frame, 2);
  for (let i = 0; i < payload.length; i++) frame[6 + i] = payload[i] ^ mask[i % 4];
  return frame;
}

function unmaskedTextFrame(text: string) {
  const payload = Buffer.from(text);
  return Buffer.concat([Buffer.from([0x81, payload.length]), payload]);
}

function readServerTextFrame(buf: Buffer) {
  const start = buf.indexOf(Buffer.from([0x81]));
  if (start < 0 || start + 2 > buf.length) return "";
  const len = buf[start + 1] & 0x7f;
  if (start + 2 + len > buf.length) return "";
  return buf.slice(start + 2, start + 2 + len).toString("utf8");
}

function readClientTextFrame(frame: Buffer) {
  if (frame.length < 6) return "";
  const len = frame[1] & 0x7f;
  const mask = frame.slice(2, 6);
  const payload = frame.slice(6, 6 + len);
  for (let i = 0; i < payload.length; i++) payload[i] = payload[i] ^ mask[i % 4];
  return payload.toString("utf8");
}

const server = createServer((_req: any, res: any) => {
  res.end("plain http");
});

server.on("upgrade", (_req: any, wsOrSocket: any, head: Buffer) => {
  // Node passes the raw socket. Perry passes a WebSocket client handle whose
  // incoming frames must be pumped even when user code imports node:http but
  // not ws directly.
  if (typeof wsOrSocket.write === "function") {
    const socket = wsOrSocket;
    socket.write(
      "HTTP/1.1 101 Switching Protocols\r\n" +
        "Upgrade: websocket\r\n" +
        "Connection: Upgrade\r\n" +
        "Sec-WebSocket-Accept: " +
        acceptValue(KEY) +
        "\r\n\r\n",
    );
    const handleFrame = (frame: Buffer) => {
      const msg = readClientTextFrame(frame);
      if (!msg) return;
      console.log("server-message:", msg);
      socket.write(unmaskedTextFrame("echo:" + msg));
    };
    if (head && head.length > 0) handleFrame(head);
    socket.on("data", handleFrame);
    return;
  }

  wsOrSocket.on("message", (msg: string) => {
    console.log("server-message:", msg);
    wsOrSocket.send("echo:" + msg);
  });
});

server.listen(PORT);

const client = connect(PORT, "127.0.0.1", () => {
  client.write(
    "GET / HTTP/1.1\r\n" +
      "Host: 127.0.0.1:" +
      PORT +
      "\r\n" +
      "Upgrade: websocket\r\n" +
      "Connection: Upgrade\r\n" +
      "Sec-WebSocket-Key: " +
      KEY +
      "\r\n" +
      "Sec-WebSocket-Version: 13\r\n\r\n",
  );
});

let handshakeDone = false;
let pending = Buffer.alloc(0);
client.on("data", (chunk: Buffer) => {
  pending = Buffer.concat([pending, chunk]);
  if (!handshakeDone) {
    const marker = pending.indexOf("\r\n\r\n");
    if (marker < 0) return;
    handshakeDone = true;
    pending = pending.slice(marker + 4);
    client.write(maskedTextFrame("hello"));
  }
  const text = readServerTextFrame(pending);
  if (text) {
    console.log("client-frame:", text);
    client.end();
    server.close();
  }
});

setTimeout(() => {}, 2000);
