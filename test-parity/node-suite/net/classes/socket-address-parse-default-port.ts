import { SocketAddress } from "node:net";

const cases = [
  {
    input: "127.0.0.1:80",
    address: "127.0.0.1",
    family: "ipv4",
  },
  {
    input: "[::1]:80",
    address: "::1",
    family: "ipv6",
  },
] as const;

for (const { input, address, family } of cases) {
  const parsed = SocketAddress.parse(input);
  const explicit = new SocketAddress({ address, family, port: 80 });
  console.log(family, parsed?.family, parsed?.port, explicit.port);
}
