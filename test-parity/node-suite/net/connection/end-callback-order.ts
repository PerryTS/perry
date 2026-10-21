import * as net from "node:net";

const server = net.createServer((socket) => socket.resume());
const events: string[] = [];
let client: net.Socket | undefined;
let originalTimer: ReturnType<typeof setTimeout> | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = net.connect((server.address() as any).port, "127.0.0.1");
  client.on("connect", () => {
    events.push("connect");
    client!.end("x", () => events.push("end-callback"));
  });
  client.on("finish", () => events.push("finish"));
  client.on("close", () => events.push("close"));
  client.on("error", () => events.push("error"));
  await new Promise<void>((resolve, reject) => {
    originalTimer = setTimeout(() => reject(new Error("original end timed out")), 3000);
    client!.once("close", resolve);
  });
  console.log(events.join(","));
} finally {
  if (originalTimer !== undefined) clearTimeout(originalTimer);
  client?.destroy();
  client?.removeAllListeners();
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}

// Each case owns a numeric loopback listener and completes at both public ends.
async function overload(label: string, connecting: boolean, form: string) {
  const events: string[] = [];
  const accepted: net.Socket[] = [];
  let received = "";
  let callbacks = 0;
  let finishes = 0;
  let closes = 0;
  let client: net.Socket | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let receivedEnd!: () => void;
  const ended = new Promise<void>((resolve) => { receivedEnd = resolve; });
  const server = net.createServer((socket) => {
    accepted.push(socket);
    socket.on("data", (chunk) => { received += chunk.toString(); });
    socket.on("end", receivedEnd);
    socket.on("error", () => events.push("server-error"));
  });
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    client = net.connect((server.address() as any).port, "127.0.0.1");
    const callback = () => {
      callbacks++;
      events.push("end-callback");
    };
    const send = () => {
      if (form === "callback") client!.end(callback);
      else if (form === "data") client!.end("x", callback);
      else if (form === "encoding") client!.end("x", "utf8", callback);
      else {
        client!.end("x");
        // Identical closures still own two distinct end registrations.
        client!.end(callback);
        client!.end(callback);
      }
    };
    client.on("connect", () => {
      events.push("connect");
      if (!connecting) send();
    });
    client.on("finish", () => { finishes++; events.push("finish"); });
    client.on("close", () => { closes++; events.push("close"); });
    client.on("error", () => events.push("error"));
    const closed = new Promise<void>((resolve) => client!.once("close", resolve));
    const bounded = new Promise<void>((_, reject) => {
      timer = setTimeout(() => reject(new Error(label + " timed out")), 3000);
    });
    if (connecting) send();
    await Promise.race([Promise.all([closed, ended]), bounded]);
    console.log(label, JSON.stringify(received), callbacks, finishes, closes, events.join(","));
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    client?.destroy();
    client?.removeAllListeners();
    for (const socket of accepted) socket.destroy();
    if (server.listening) {
      await new Promise<void>((resolve) => server.close(() => resolve()));
    }
    server.removeAllListeners();
  }
}

await overload("connected-callback", false, "callback");
await overload("connected-data", false, "data");
await overload("connected-encoding-utf8", false, "encoding");
await overload("connected-repeated", false, "repeated");
await overload("connecting-callback", true, "callback");
await overload("connecting-data", true, "data");
await overload("connecting-encoding-utf8", true, "encoding");
await overload("connecting-repeated", true, "repeated");
