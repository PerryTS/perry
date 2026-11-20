import net from "node:net";

// Every case owns its listener, both endpoints and a hard teardown deadline.
async function mutation(label: string, initial: boolean, value: any, inEnd = false) {
  let client: any;
  let peer: any;
  let endedInside = false;
  const server = net.createServer({ allowHalfOpen: true }, (socket) => { peer = socket; });
  const deadline = setTimeout(() => { console.error("deadline", label); process.exit(1); }, 6000);
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    client = new net.Socket({ allowHalfOpen: initial });
    const connection = new Promise<void>((resolve) => server.once("connection", () => resolve()));
    await new Promise<void>((resolve, reject) => {
      client.once("error", reject);
      client.connect({ host: "127.0.0.1", port: (server.address() as any).port }, resolve);
    });
    await connection;
    console.log(label, "child", peer.allowHalfOpen);
    if (!inEnd) client.allowHalfOpen = value;
    client.resume();
    await new Promise<void>((resolve) => {
      client.once("end", () => {
        endedInside = client.writableEnded;
        if (inEnd) client.allowHalfOpen = value;
        setImmediate(resolve);
      });
      peer.end("payload");
    });
    console.log(label, "ended", endedInside, client.writableEnded);
  } finally {
    clearTimeout(deadline);
    client?.destroy();
    peer?.destroy();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}

await mutation("false-true", false, true);
await mutation("true-false", true, false);
await mutation("zero", true, 0);
await mutation("null", true, null);
await mutation("undefined", true, undefined);
await mutation("empty", true, "");
await mutation("end-true", false, true, true);
await mutation("end-false", true, false, true);

async function reopen(fromGetter: boolean) {
  let client: any;
  let peer: any;
  let userEnd = false;
  let retired = false;
  let getterRetired = false;
  let phase = 0;
  const label = fromGetter ? "getter-reopen" : "end-reopen";
  const server = net.createServer({ allowHalfOpen: true }, (socket) => { peer = socket; });
  const deadline = setTimeout(() => { console.error("deadline", label); process.exit(1); }, 6000);
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject); server.listen(0, "127.0.0.1", resolve);
    });
    client = new net.Socket({ allowHalfOpen: true });
    const options = { host: "127.0.0.1", port: (server.address() as any).port };
    const connection = new Promise<void>((resolve) => server.once("connection", () => resolve()));
    await new Promise<void>((resolve, reject) => {
      client.once("error", reject); client.connect(options, resolve);
    });
    await connection;
    if (fromGetter) Object.defineProperty(client, "allowHalfOpen", {
      configurable: true, enumerable: true,
      get() {
        if (userEnd && !retired) { retired = true; getterRetired = true; client.destroy(); }
        return false;
      },
    });
    client.resume();
    await new Promise<void>((resolve) => {
      client.once("close", resolve);
      client.once("end", () => {
        userEnd = true;
        if (!fromGetter) { retired = true; client.destroy(); }
      });
      peer.end("first");
    });
    Object.defineProperty(client, "allowHalfOpen", {
      value: "preserved", writable: true, configurable: true, enumerable: true,
    });
    const reconnected = new Promise<void>((resolve) => server.once("connection", () => resolve()));
    await new Promise<void>((resolve, reject) => {
      client.once("error", reject); client.connect(options, resolve);
    });
    await reconnected;
    phase++;
    await new Promise<void>((resolve) => setImmediate(resolve));
    console.log(label, userEnd, retired, getterRetired, phase,
      client.allowHalfOpen, client.writableEnded, client.destroyed);
  } finally {
    clearTimeout(deadline); client?.destroy(); peer?.destroy();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}
await reopen(false);
await reopen(true);

// Accepted children inherit listener defaults, not another Socket's property.
let child: any;
const server = net.createServer({ allowHalfOpen: false }, (socket) => { child = socket; });
const deadline = setTimeout(() => { console.error("deadline", "child-false"); process.exit(1); }, 6000);
let client: any;
try {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const connection = new Promise<void>((resolve) => server.once("connection", () => resolve()));
  client = new net.Socket({ allowHalfOpen: true });
  await new Promise<void>((resolve, reject) => {
    client.once("error", reject);
    client.connect({ host: "127.0.0.1", port: (server.address() as any).port }, resolve);
  });
  await connection;
  console.log("child-false", child.allowHalfOpen, client.allowHalfOpen);
} finally {
  clearTimeout(deadline); client?.destroy(); child?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
