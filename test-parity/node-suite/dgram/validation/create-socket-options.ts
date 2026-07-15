import * as dgram from "node:dgram";

function errorOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (error: unknown) {
    const value = error as { code?: string; name?: string };
    return `${value.name}:${value.code}`;
  }
}

const invalidTypes: unknown[] = [
  "udp5",
  ["udp4"],
  new String("udp4"),
  1,
  {},
  true,
  false,
  null,
  undefined,
];

console.log(
  "invalid types:",
  invalidTypes.map((type) => errorOf(() => dgram.createSocket(type as never))).join(","),
);

const udp4 = dgram.createSocket({ type: "udp4" });
const udp6 = dgram.createSocket({ type: "udp6" });
console.log("valid option types:", typeof udp4.send, typeof udp6.send);
await Promise.all([
  new Promise<void>((resolve) => udp4.close(() => resolve())),
  new Promise<void>((resolve) => udp6.close(() => resolve())),
]);
