import * as http2 from "node:http2";

// Each server session's 'close' comes from its socket's close callback:
// newest first, each followed by its ticks, and a due timer runs after.
const server = http2.createServer();
const sessions: http2.ServerHttp2Session[] = [];
const bothOpen = new Promise<void>((resolve) => {
  server.on("session", (session) => {
    if (sessions.push(session) === 2) resolve();
  });
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const url = `http://127.0.0.1:${(server.address() as any).port}`;
const clients = [http2.connect(url), http2.connect(url)];
for (const client of clients) client.on("error", () => {});
await bothOpen;

const [a, b] = sessions;
const closed = new Promise<void>((resolve) => {
  let left = 2;
  for (const [name, session] of [["a", a], ["b", b]] as const) {
    session.on("close", () => {
      console.log(`close ${name}`, session.closed, session.destroyed);
      process.nextTick(() => console.log(`tick ${name}`));
      if (--left === 0) resolve();
    });
  }
});
setTimeout(() => console.log("timeout"), 0);
a.destroy();
b.destroy();
console.log("destroyed", a.destroyed, b.destroyed);
await closed;
await new Promise((resolve) => setTimeout(resolve, 10));
for (const client of clients) client.destroy();
await new Promise<void>((resolve) => server.close(() => resolve()));
console.log("done");
