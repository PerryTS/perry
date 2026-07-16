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
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const sender = dgram.createSocket("udp4");
const invalidAddresses: unknown[] = [[], 0, 1, true, false, 0n, 1n, {}, Symbol("address")];

console.log(
  "invalid addresses:",
  invalidAddresses
    .map((address) =>
      codeOf(() => sender.send("x", receiver.address().port, address as never))
    )
    .join(","),
);

await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
