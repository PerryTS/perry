// Node's `autoSelectFamily` connect: every address a lookup returns is tried
// in turn, each attempt reported through 'connectionAttempt*' events, a
// pending attempt abandoned after `autoSelectFamilyAttemptTimeout`, and an
// AggregateError of every attempt's error when all of them fail.
import * as net from "node:net";

const server = net.createServer((socket) => socket.end());
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as net.AddressInfo).port;
const closed = net.createServer();
await new Promise<void>((resolve) => closed.listen(0, "127.0.0.1", resolve));
const closedPort = (closed.address() as net.AddressInfo).port;
await new Promise((resolve) => closed.close(resolve));
// TEST-NET-1 is never routed: an attempt there either hangs or fails at once.
const blackhole = "192.0.2.1";

const show = (text: string) =>
  text.replaceAll(`:${port}`, ":<port>").replaceAll(`:${closedPort}`, ":<closed>");

function attempt(label: string, options: any, answer?: any[]): Promise<void> {
  return new Promise((resolve) => {
    const socket = net.connect({
      ...options,
      ...(answer && {
        host: "eyeballs.test",
        lookup(_host: string, dnsOptions: any, callback: any) {
          setTimeout(() => {
            if (dnsOptions.all) callback(null, answer);
            else callback(null, answer[0].address, answer[0].family);
          }, 1);
        },
      }),
    });
    socket.on("connectionAttempt", (address: string, attemptPort: number, family: number) => {
      console.log(label, "attempt", address, attemptPort === socket.remotePort || attemptPort === options.port, family);
    });
    socket.on("connectionAttemptFailed", (address: string, _port: number, family: number, err: any) => {
      if (address === blackhole) console.log(label, "blackhole abandoned");
      else console.log(label, "attempt failed", address, family, err.code, show(err.message));
    });
    socket.on("connectionAttemptTimeout", (address: string) => {
      if (address === blackhole) console.log(label, "blackhole abandoned");
      else console.log(label, "attempt timeout", address);
    });
    socket.on("connect", () => {
      console.log(label, "connect", socket.remoteAddress);
      socket.end();
    });
    socket.on("error", (err: any) => {
      console.log(label, "error", err.name, JSON.stringify(show(err.message)), err.code);
      for (const inner of err.errors ?? []) console.log(label, "  inner", inner.code, show(inner.message));
    });
    socket.on("close", () => {
      const attempted = (socket as any).autoSelectFamilyAttemptedAddresses;
      console.log(label, "attempted", attempted && show(attempted.join(" ")));
      resolve();
    });
  });
}

const v4 = { address: "127.0.0.1", family: 4 };
const v6 = { address: "::1", family: 6 };
await attempt("fallback", { port }, [v6, v4]);
await attempt("refused", { port: closedPort }, [v6, v4]);
await attempt("timeout", { port, autoSelectFamilyAttemptTimeout: 20 }, [{ address: blackhole, family: 4 }, v4]);
await attempt("off", { port, autoSelectFamily: false }, [v4, v6]);
await attempt("ip", { port, host: "127.0.0.1" });
await attempt("ip-refused", { port: closedPort, host: "127.0.0.1" });

// Destroying while an attempt is pending stops the race: its timer never
// fires and nothing is reported after the close.
const stopped = net.connect({
  port,
  host: "eyeballs.test",
  autoSelectFamilyAttemptTimeout: 200,
  lookup(_host: string, _options: any, callback: any) {
    const second = { address: "192.0.2.2", family: 4 };
    setTimeout(() => callback(null, [{ address: blackhole, family: 4 }, second]), 1);
  },
});
// Neither TEST-NET address can connect, so the destroy below is never late.
stopped.once("connectionAttempt", (address: string) => {
  console.log("stopped attempt", address === blackhole ? "blackhole" : address);
  setTimeout(() => stopped.destroy(), 20);
});
stopped.on("error", (err: any) => console.log("stopped error", err.name));
stopped.on("connectionAttemptTimeout", () => console.log("stopped timeout"));
stopped.on("connectionAttemptFailed", () => console.log("stopped failed"));
stopped.on("connect", () => console.log("stopped connect"));
stopped.on("close", () => console.log("stopped close"));
await new Promise((resolve) => setTimeout(resolve, 400));
console.log("stopped", stopped.destroyed);

server.close();
