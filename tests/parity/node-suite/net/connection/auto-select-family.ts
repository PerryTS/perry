// Node's `autoSelectFamily` connect: every address a lookup returns is tried
// in turn, each attempt reported through 'connectionAttempt*' events, and an
// AggregateError of every attempt's error when all of them fail. Only
// addresses whose outcome is fixed on the host are used: the IPv4 listener
// and a port nothing listens on. Attempt timeouts are covered by the runtime's
// transport tests, which can stall an attempt deterministically.
import * as net from "node:net";

const server = net.createServer((socket) => socket.end());
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as net.AddressInfo).port;
const closed = net.createServer();
await new Promise<void>((resolve) => closed.listen(0, "127.0.0.1", resolve));
const closedPort = (closed.address() as net.AddressInfo).port;
await new Promise((resolve) => closed.close(resolve));

const show = (text: string) =>
  text.replaceAll(`:${port}`, ":<port>").replaceAll(`:${closedPort}`, ":<closed>");

const v4 = { address: "127.0.0.1", family: 4 };
const v6 = { address: "::1", family: 6 };

function attempt(
  label: string,
  options: any,
  answer?: any[],
  hooks: (socket: net.Socket) => void = () => {},
  afterAnswer: (socket: net.Socket) => void = () => {},
): Promise<void> {
  return new Promise((resolve) => {
    const socket: net.Socket = net.connect({
      ...options,
      ...(answer && {
        host: "eyeballs.test",
        lookup(_host: string, dnsOptions: any, callback: any) {
          setTimeout(() => {
            if (dnsOptions.all) callback(null, answer);
            else callback(null, answer[0].address, answer[0].family);
            afterAnswer(socket);
          }, 1);
        },
      }),
    });
    socket.on("connectionAttempt", (address: string, _port: number, family: number) => {
      console.log(label, "attempt", address, family);
    });
    socket.on("connectionAttemptFailed", (address: string, _port: number, family: number, err: any) => {
      console.log(label, "attempt failed", address, family, err.code, show(err.message));
    });
    socket.on("connectionAttemptTimeout", (address: string) => {
      console.log(label, "attempt timeout", address);
    });
    socket.on("connect", () => {
      console.log(label, "connect", socket.remoteAddress);
      socket.end();
    });
    socket.on("error", (err: any) => {
      console.log(label, "error", err.name, JSON.stringify(show(err.message)), err.code);
      for (const inner of err.errors ?? []) console.log(label, "  inner", inner.code, show(inner.message));
    });
    socket.on("close", (hadError: boolean) => {
      const attempted = (socket as any).autoSelectFamilyAttemptedAddresses;
      console.log(label, "close", hadError, "attempted", attempted && show(attempted.join(" ")));
      resolve();
    });
    hooks(socket);
  });
}

await attempt("fallback", { port }, [v6, v4]);
await attempt("refused", { port: closedPort }, [v6, v4]);
await attempt("off", { port, autoSelectFamily: false }, [v4, v6]);
await attempt("ip", { port, host: "127.0.0.1" });
await attempt("ip-refused", { port: closedPort, host: "127.0.0.1" });

// A destroy from a failure listener cancels nothing more.
await attempt("destroy-on-failure", { port }, [v6, v4], (socket) => {
  socket.on("connectionAttemptFailed", () => socket.destroy());
});
// Destroying while the first attempt is pending cancels it: the destroy's
// own 'error' comes first, then the cancelled attempt.
await attempt("destroy-pending", { port }, [v6, v4], undefined, (socket) => socket.destroy());
await attempt("destroy-pending-error", { port }, [v6, v4], undefined, (socket) => {
  socket.destroy(new Error("boom"));
});

// A socket destroyed before its IP connect runs reports no attempt.
const early = net.connect(port, "127.0.0.1");
early.on("connectionAttempt", () => console.log("early attempt"));
early.destroy();
await new Promise((resolve) => early.on("close", resolve));
console.log("early closed");

for (const value of ["10", 15.5, 0, 1e12, 2 ** 31, NaN]) {
  try {
    const socket = net.connect({ port, host: "127.0.0.1", autoSelectFamilyAttemptTimeout: value as any });
    socket.on("error", () => {});
    socket.destroy();
    console.log("timeout", value, "accepted");
  } catch (err: any) {
    console.log("timeout", err.name, err.code, err.message);
  }
}

server.close();
