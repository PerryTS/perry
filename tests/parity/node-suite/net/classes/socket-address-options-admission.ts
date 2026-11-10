import { SocketAddress } from "node:net";
import { Buffer } from "node:buffer";

const cases: [string, () => SocketAddress][] = [
  ["omitted", () => new SocketAddress()],
  ["undefined", () => new SocketAddress(undefined)],
  ["object", () => new SocketAddress({})],
  ["prototype", () => new SocketAddress(Object.create({}))],
  ["date", () => new SocketAddress(new Date(0) as any)],
  ["buffer", () => new SocketAddress(Buffer.alloc(0) as any)],
  ["null", () => new SocketAddress(null as any)],
  ["number", () => new SocketAddress(1 as any)],
  ["string", () => new SocketAddress("bad" as any)],
  ["boolean", () => new SocketAddress(true as any)],
  ["array", () => new SocketAddress([] as any)],
  ["function", () => new SocketAddress((() => {}) as any)],
  // Run with the harness's process timeout: numeric bits must not be pointers.
  ["subnormal", () => new SocketAddress(1e-310 as any)],
];

for (const [label, create] of cases) {
  try {
    const address = create();
    console.log(label, address.address, address.family, address.port, address.flowlabel);
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}
