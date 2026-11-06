import { SocketAddress } from "node:net";

for (
  const options of [undefined, {}, { family: "IPv6" }, {
    family: "IPV4",
  }] as any[]
) {
  try {
    const address = new SocketAddress(options);
    console.log(
      JSON.stringify(options),
      address.address,
      address.family,
      address.port,
      address.flowlabel,
      JSON.stringify(address.toJSON()),
      SocketAddress.isSocketAddress(address),
    );
  } catch (error: any) {
    console.log(JSON.stringify(options), error.name, error.code);
  }
}

console.log("plain object:", SocketAddress.isSocketAddress({}));
