// A failed listen() emits a real Error; with no 'error' listener it is thrown,
// so it reaches 'uncaughtException' (and exits 1 without one).
import http from "node:http";
import http2 from "node:http2";
import net from "node:net";

let port = 0;
let uncaught: (error: any) => void = () => {};
process.on("uncaughtException", (error) => uncaught(error));

const show = (label: string, error: any) => {
  console.log(label, error instanceof Error, error.code, error.syscall, error.address, error.port === port);
  console.log(error.message.replace(String(port), "PORT"), error.errno < 0, Object.keys(error).join());
};

const probe = (label: string, create: () => any) =>
  new Promise<void>((resolve) => {
    uncaught = (error) => {
      show(`${label} uncaught`, error);
      uncaught = (late) => console.log("unexpected", late);
      create()
        .on("error", (e: any) => {
          show(`${label} listener`, e);
          resolve();
        })
        .listen(port, "127.0.0.1", () => console.log("unexpected listening"));
    };
    create().listen(port, "127.0.0.1", () => console.log("unexpected listening"));
  });

const holder = http.createServer();
holder.listen(0, "127.0.0.1", async () => {
  port = (holder.address() as any).port;
  await probe("http", () => http.createServer());
  await probe("http2", () => http2.createServer());
  await probe("net", () => net.createServer());
  holder.close(() => console.log("done"));
});
