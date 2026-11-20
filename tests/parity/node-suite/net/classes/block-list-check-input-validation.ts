import { BlockList, SocketAddress } from "node:net";

const list = new BlockList();
list.addAddress("127.0.0.1");
const savedCheck = list.check;
const socketAddress = SocketAddress.parse("127.0.0.1:8080");

function reportError(route: string, label: string, error: any) {
  const message = String(error.message);
  const argument = message.includes('"address"')
    ? "address"
    : message.includes('"family"')
    ? "family"
    : "other";
  console.log(route, label, "throw", error.name, error.code, argument);
}

console.log("saved check:", typeof savedCheck);

for (const [label, address, family] of [
  ["invalid text", "bad", undefined],
  ["missing address", undefined, undefined],
  ["null address", null, undefined],
  ["number address", 123, undefined],
  ["boolean address", true, undefined],
  ["object address", {}, undefined],
  ["family type", "127.0.0.1", 123],
  ["family string", "127.0.0.1", "nope"],
  ["undefined family", "127.0.0.1", undefined],
  ["address precedence", 123, 123],
  ["family before parse", "bad", 123],
  ["SocketAddress", socketAddress, undefined],
  ["SocketAddress family", socketAddress, "ipv6"],
  ["SocketAddress ignored type", socketAddress, 123],
] as [string, any, any][]) {
  try {
    console.log("direct", label, "return", list.check(address, family));
  } catch (error: any) {
    reportError("direct", label, error);
  }
  try {
    console.log("saved", label, "return", savedCheck.call(list, address, family));
  } catch (error: any) {
    reportError("saved", label, error);
  }
}

try {
  console.log("direct", "omitted family", "return", list.check("127.0.0.1"));
} catch (error: any) {
  reportError("direct", "omitted family", error);
}
try {
  console.log("saved", "omitted family", "return", savedCheck.call(list, "127.0.0.1"));
} catch (error: any) {
  reportError("saved", "omitted family", error);
}

const erased: any = list;
const checkKey = "check";
const prototypeCheck = Object.getPrototypeOf(list).check;
let coercions = 0;
const poison = {
  toJSON() {
    coercions++;
    return "127.0.0.1";
  },
};

console.log("prototype check:", typeof prototypeCheck);
for (const [label, address, family] of [
  ["number", 123, undefined],
  ["poison", poison, undefined],
  ["SocketAddress", socketAddress, 123],
] as [string, any, any][]) {
  try {
    console.log("computed", label, "return", list[checkKey](address, family));
  } catch (error: any) {
    reportError("computed", label, error);
  }
  try {
    console.log("erased", label, "return", erased.check(address, family));
  } catch (error: any) {
    reportError("erased", label, error);
  }
  try {
    console.log("prototype", label, "return", prototypeCheck.call(list, address, family));
  } catch (error: any) {
    reportError("prototype", label, error);
  }
}
console.log("coercions:", coercions);
