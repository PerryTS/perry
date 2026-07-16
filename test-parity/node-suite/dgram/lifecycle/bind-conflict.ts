import * as dgram from "node:dgram";

const first = dgram.createSocket("udp4");
await new Promise<void>((resolve) => first.bind(0, "127.0.0.1", () => resolve()));

const second = dgram.createSocket("udp4");
const error = new Promise<string>((resolve) => {
  second.once("error", (value) => resolve(`${value.code}:${value.syscall}`));
});
second.bind(first.address().port, "127.0.0.1");
console.log("bind conflict:", await error);

await Promise.all([
  new Promise<void>((resolve) => first.close(() => resolve())),
  new Promise<void>((resolve) => second.close(() => resolve())),
]);
