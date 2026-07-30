import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", resolve));

const sender = dgram.createSocket("udp4");
const order: string[] = [];

try {
  sender.once("connect", () => order.push("event"));

  await new Promise<void>((resolve) => {
    sender.connect(receiver.address().port, "127.0.0.1", function () {
      order.push(`callback:${this === sender}:${arguments.length}`);
      resolve();
    });
  });

  const remote = sender.remoteAddress();
  console.log("connect order:", order.join(","));
  console.log(
    "remote:",
    remote.address,
    remote.family,
    remote.port === receiver.address().port,
  );
} finally {
  const senderClosed = new Promise<void>((resolve) =>
    sender.once("close", resolve)
  );
  const receiverClosed = new Promise<void>((resolve) =>
    receiver.once("close", resolve)
  );
  sender.close();
  receiver.close();
  await Promise.all([senderClosed, receiverClosed]);
}
