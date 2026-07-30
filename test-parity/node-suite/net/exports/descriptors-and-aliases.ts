import * as net from "node:net";

for (
  const name of [
    "connect",
    "createConnection",
    "createServer",
    "Server",
    "Socket",
    "Stream",
    "BlockList",
    "SocketAddress",
    "isIP",
    "isIPv4",
    "isIPv6",
  ]
) {
  const descriptor = Object.getOwnPropertyDescriptor(net, name);
  console.log(
    name,
    typeof (net as any)[name],
    descriptor?.enumerable,
    descriptor?.configurable,
  );
}

console.log("connect alias:", net.connect === net.createConnection);
console.log("stream alias:", net.Stream === net.Socket);
