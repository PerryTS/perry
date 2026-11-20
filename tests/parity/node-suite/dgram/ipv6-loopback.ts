// Upstream: Node v26.5.0 test/parallel/test-dgram-address.js and test-dgram-udp6-send-default-host.js.
// Coverage added: capability-guarded IPv6 loopback delivery.
import * as dgram from "node:dgram";

const socket = dgram.createSocket("udp6");
let bound = false;
try {
  const bindResult = await new Promise<string>((resolve) => {
    const onError = (error: Error & { code?: string }) => {
      socket.removeListener("listening", onListening);
      resolve(error.code ?? "Error");
    };
    const onListening = () => {
      socket.removeListener("error", onError);
      resolve("listening");
    };
    socket.once("error", onError);
    socket.once("listening", onListening);
    socket.bind(0, "::1");
  });
  bound = bindResult === "listening";
  console.log("ipv6 bind:", bindResult);

  if (bound) {
    const address = socket.address();
    const received = new Promise<string>((resolve) => {
      socket.once("message", (message, rinfo) => {
        resolve(
          `${message.toString()}:${rinfo.address}:${rinfo.family}:${rinfo.size}`,
        );
      });
    });
    const sent = new Promise<string>((resolve) => {
      socket.send("v6", address.port, "::1", (error, bytes) => {
        resolve(`${error === null}:${bytes}`);
      });
    });
    console.log(
      "ipv6 address:",
      address.address,
      address.family,
      address.port > 0,
    );
    console.log("ipv6 delivery:", await received, await sent);
  }
} finally {
  await new Promise<void>((resolve) => socket.close(resolve));
}
