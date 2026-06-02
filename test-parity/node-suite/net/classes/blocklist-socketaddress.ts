import * as net from "node:net";

function firstLine(err: any): string {
  return String(err.message).split("\n")[0];
}

function logThrow(label: string, fn: () => unknown) {
  try {
    const result = fn();
    console.log(label, "OK", result === undefined ? "undefined" : String(result));
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, "|", firstLine(err));
  }
}

console.log("BlockList export:", typeof net.BlockList, net.BlockList.length);
const blockList = new net.BlockList();
console.log(
  "BlockList identity:",
  typeof blockList,
  net.BlockList.isBlockList(blockList),
  net.BlockList.isBlockList({}),
);
console.log(
  "BlockList methods:",
  [
    "addAddress",
    "addRange",
    "addSubnet",
    "check",
    "toJSON",
    "fromJSON",
  ].map((name) => `${name}:${typeof (blockList as any)[name]}`).join(","),
);

blockList.addAddress("127.0.0.1");
blockList.addRange("10.0.0.1", "10.0.0.10");
blockList.addSubnet("192.168.0.0", 16);
console.log("BlockList rules:", JSON.stringify(blockList.rules));
console.log(
  "BlockList checks:",
  blockList.check("127.0.0.1"),
  blockList.check("10.0.0.5"),
  blockList.check("192.168.2.10"),
  blockList.check("8.8.8.8"),
);

const roundTrip = new net.BlockList();
console.log("BlockList fromJSON return:", roundTrip.fromJSON(blockList.toJSON()));
console.log(
  "BlockList fromJSON:",
  roundTrip.check("127.0.0.1"),
  roundTrip.check("10.0.0.5"),
  JSON.stringify(roundTrip.rules),
);
logThrow("BlockList bad address", () => blockList.addAddress("not-ip"));
logThrow("BlockList bad subnet", () => blockList.addSubnet("127.0.0.1", 33));

console.log("SocketAddress export:", typeof net.SocketAddress, net.SocketAddress.length);
const address = new net.SocketAddress({
  address: "127.0.0.1",
  port: 80,
  family: "ipv4",
});
console.log(
  "SocketAddress fields:",
  address.address,
  address.family,
  address.port,
  address.flowlabel,
);

const address6 = new net.SocketAddress({
  address: "::1",
  family: "ipv6",
  port: 443,
  flowlabel: 123,
});
console.log(
  "SocketAddress ipv6:",
  address6.address,
  address6.family,
  address6.port,
  address6.flowlabel,
);

const parsed4 = net.SocketAddress.parse("127.0.0.1:8080");
const parsed6 = net.SocketAddress.parse("[::1]:8443");
console.log(
  "SocketAddress parse4:",
  parsed4 && `${parsed4.address}|${parsed4.family}|${parsed4.port}|${parsed4.flowlabel}`,
);
console.log(
  "SocketAddress parse6:",
  parsed6 && `${parsed6.address}|${parsed6.family}|${parsed6.port}|${parsed6.flowlabel}`,
);
console.log("SocketAddress parse bad:", net.SocketAddress.parse("not an address"));
logThrow(
  "SocketAddress bad port",
  () => new net.SocketAddress({ address: "127.0.0.1", port: 70000 }),
);
logThrow(
  "SocketAddress bad family",
  () => new net.SocketAddress({ address: "127.0.0.1", port: 80, family: "ipv6" }),
);
