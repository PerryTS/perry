// Upstream: Node v26.5.0 test/parallel/test-dgram-bind-sync.js.
// Coverage added: Node 26's synchronous bind contract.
import * as dgram from "node:dgram";

type BindSync = (options?: { address?: string; port?: number }) => {
  address: string;
  family: string;
  port: number;
};
const socket = dgram.createSocket("udp4");
const bindSync = (socket as dgram.Socket & { bindSync?: BindSync }).bindSync;

if (typeof bindSync !== "function") {
  console.log("bindSync supported:", false);
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  await new Promise<void>((resolve) => socket.close(resolve));
} else {
  const order: string[] = [];
  const listening = new Promise<void>((resolve) =>
    socket.once("listening", () => {
      order.push("listening");
      resolve();
    })
  );
  try {
    const address = bindSync.call(socket, { address: "127.0.0.1", port: 0 });
    order.push("returned");
    console.log("bindSync supported:", true);
    console.log(
      "bindSync address:",
      address.address,
      address.family,
      address.port > 0,
    );
    console.log("bindSync state:", socket.address().port === address.port);
    await listening;
    console.log("bindSync order:", order.join(","));
  } finally {
    await new Promise<void>((resolve) => socket.close(resolve));
  }
}
