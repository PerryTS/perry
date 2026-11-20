import { SocketAddress } from "node:net";

for (const input of ["127.0.0.1:8080", "[::1]:8443", "not an address"]) {
  const parsed = SocketAddress.parse(input);
  console.log(
    input,
    parsed?.address,
    parsed?.family,
    parsed?.port,
    parsed?.flowlabel,
  );
}
