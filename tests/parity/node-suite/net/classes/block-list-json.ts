import { BlockList } from "node:net";

const rules = ["Address: IPv4 127.0.0.1", "Subnet: IPv6 2001:db8::/32"];
console.log(
  "methods:",
  typeof BlockList.prototype.toJSON,
  typeof BlockList.prototype.fromJSON,
);

if (typeof BlockList.prototype.fromJSON === "function") {
  for (const value of [rules, JSON.stringify(rules)] as any[]) {
    const list = new BlockList();
    console.log("return:", list.fromJSON(value));
    console.log(
      "checks:",
      list.check("127.0.0.1"),
      list.check("2001:db8::1", "ipv6"),
    );
  }

  for (const value of [null, {}, [1], "{}"] as any[]) {
    try {
      new BlockList().fromJSON(value);
      console.log("invalid", JSON.stringify(value), "OK");
    } catch (error: any) {
      console.log("invalid", JSON.stringify(value), error.name, error.code);
    }
  }
}
