import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", resolve));

const first = dgram.createSocket("udp4");
const second = dgram.createSocket("udp4");
const received: string[] = [];
const messages = new Promise<void>((resolve) => {
  receiver.on("message", (message) => {
    received.push(message.toString());
    if (received.length === 2) resolve();
  });
});

await Promise.all([
  new Promise<void>((resolve, reject) => {
    first.send("first", receiver.address().port, "127.0.0.1", (error) =>
      error ? reject(error) : resolve()
    );
  }),
  new Promise<void>((resolve, reject) => {
    second.send("second", receiver.address().port, "127.0.0.1", (error) =>
      error ? reject(error) : resolve()
    );
  }),
  messages,
]);

const firstAddress = first.address();
const secondAddress = second.address();
console.log("messages:", received.sort().join(","));
console.log("families:", firstAddress.family, secondAddress.family);
console.log("ports assigned:", firstAddress.port > 0, secondAddress.port > 0);
console.log("ports distinct:", firstAddress.port !== secondAddress.port);

await new Promise<void>((resolve) => first.close(resolve));
await new Promise<void>((resolve) => second.close(resolve));
await new Promise<void>((resolve) => receiver.close(resolve));
