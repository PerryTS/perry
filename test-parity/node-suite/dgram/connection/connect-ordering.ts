import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", resolve));

const sender = dgram.createSocket("udp4");
const order: string[] = [];
sender.once("connect", () => order.push("event"));

await new Promise<void>((resolve) => {
  sender.connect(receiver.address().port, "127.0.0.1", () => {
    order.push("callback");
    resolve();
  });
});

const remote = sender.remoteAddress();
console.log("connect order:", order.join(","));
console.log("remote:", remote.address, remote.family, remote.port === receiver.address().port);

await Promise.all([
  new Promise<void>((resolve) => sender.close(resolve)),
  new Promise<void>((resolve) => receiver.close(resolve)),
]);
