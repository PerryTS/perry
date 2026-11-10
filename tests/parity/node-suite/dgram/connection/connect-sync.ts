// Upstream: Node v26.5.0 test/parallel/test-dgram-connect-sync.js.
// Coverage added: Node 26's synchronous connect contract.
import * as dgram from "node:dgram";

type ConnectSync = (port: number, address?: string) => void;
const receiver = dgram.createSocket("udp4");
const sender = dgram.createSocket("udp4");
const connectSync =
  (sender as dgram.Socket & { connectSync?: ConnectSync }).connectSync;

try {
  await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", resolve));
  if (typeof connectSync !== "function") {
    console.log("connectSync supported:", false);
  } else {
    const order: string[] = [];
    const connected = new Promise<void>((resolve) =>
      sender.once("connect", () => {
        order.push("connect");
        resolve();
      })
    );
    const result = connectSync.call(
      sender,
      receiver.address().port,
      "127.0.0.1",
    );
    order.push("returned");
    const remote = sender.remoteAddress();
    console.log("connectSync supported:", true);
    console.log("connectSync result:", result);
    console.log(
      "connectSync remote:",
      remote.address,
      remote.family,
      remote.port === receiver.address().port,
    );
    await connected;
    console.log("connectSync order:", order.join(","));
  }
} finally {
  await Promise.all([
    new Promise<void>((resolve) => sender.close(resolve)),
    new Promise<void>((resolve) => receiver.close(resolve)),
  ]);
}
