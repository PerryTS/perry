import { SocketAddress } from "node:net";

const cases: [string, () => unknown][] = [
  ["options null", () => new SocketAddress(null as any)],
  ["address type", () => new SocketAddress({ address: 1 as any })],
  ["family value", () => new SocketAddress({ family: "ip5" as any })],
  ["port negative", () => new SocketAddress({ port: -1 })],
  ["port fraction", () => new SocketAddress({ port: 1.5 })],
  [
    "flowlabel negative",
    () => new SocketAddress({ family: "ipv6", flowlabel: -1 }),
  ],
  [
    "flowlabel fraction",
    () => new SocketAddress({ family: "ipv6", flowlabel: 1.5 }),
  ],
];

for (const [label, create] of cases) {
  try {
    create();
    console.log(label, "OK");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}

for (
  const input of [1, null, {}, "bad", "127.0.0.1:80", "[::1]:443"] as any[]
) {
  try {
    const parsed = SocketAddress.parse(input);
    console.log("parse", JSON.stringify(input), parsed?.family, parsed?.port);
  } catch (error: any) {
    console.log("parse", JSON.stringify(input), error.name, error.code);
  }
}
