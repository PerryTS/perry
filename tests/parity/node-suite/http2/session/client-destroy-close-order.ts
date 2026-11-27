import * as http2 from "node:http2";

// Each client session's 'close' comes from its socket's close callback:
// newest first, each followed by its ticks, and a due timer runs after.
const server = http2.createServer();
let accepted = 0;
const bothAccepted = new Promise<void>((resolve) => {
  server.on("session", () => {
    if (++accepted === 2) resolve();
  });
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const url = `http://127.0.0.1:${(server.address() as any).port}`;
const clients = [http2.connect(url), http2.connect(url)];
for (const client of clients) client.on("error", () => {});
await Promise.all(clients.map((client) => new Promise((resolve) => client.on("connect", resolve))));
await bothAccepted;

const [a, b] = clients;
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
// The destroys run in a timers-phase callback, as in the issue's probe, so
// the close phase they owe comes before the next iteration's timers.
await new Promise<void>((resolve) =>
  setTimeout(() => {
    setTimeout(() => console.log("timeout"), 0);
    a.destroy();
    b.destroy();
    console.log("destroyed", a.closed, a.destroyed, b.closed, b.destroyed);
    resolve();
  }, 0),
);
await closed;
await new Promise((resolve) => setTimeout(resolve, 10));
await new Promise<void>((resolve) => server.close(() => resolve()));
console.log("done");
