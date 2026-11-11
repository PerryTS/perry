// Upstream: Node v26.5.0 test/parallel/test-dgram-udp6-send-default-host.js.
// Coverage added: udp6 omitted-host dispatch to ::1.
import * as dgram from "node:dgram";

const socket = dgram.createSocket("udp6");
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
  console.log("ipv6 default bind:", bindResult);

  if (bindResult === "listening") {
    const received = new Promise<string>((resolve) => {
      socket.once("message", (message, rinfo) => {
        resolve(`${message.toString()}:${rinfo.address}:${rinfo.family}`);
      });
    });
    const sent = new Promise<string>((resolve) => {
      socket.send("default-v6", socket.address().port, (error, bytes) => {
        resolve(`${error === null}:${bytes}`);
      });
    });
    console.log("ipv6 default delivery:", await received, await sent);
  }
} finally {
  await new Promise<void>((resolve) => socket.close(resolve));
}
