import * as dgram from "node:dgram";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (error: unknown) {
    return (error as { code?: string; name?: string }).code ??
      (error as { name?: string }).name ?? "Error";
  }
}

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", resolve));
const sender = dgram.createSocket("udp4");

let finishCallback: (value: string) => void = () => {};
const callback = new Promise<string>((resolve) => {
  finishCallback = resolve;
});
const result = codeOf(() => {
  sender.send([], receiver.address().port, "127.0.0.1", (error, bytes) => {
    finishCallback(`${error === null}:${bytes}`);
  });
});

console.log("empty array accepted:", result);
if (result === "none") {
  console.log("empty array callback:", await callback);
}

await Promise.all([
  new Promise<void>((resolve) => sender.close(resolve)),
  new Promise<void>((resolve) => receiver.close(resolve)),
]);
