import { BlockList } from "node:net";
import * as net from "node:net";

const proto = BlockList.prototype;
const read = proto.toJSON;
const load = proto.fromJSON;
console.log("publication", typeof read, typeof load, proto === net.BlockList.prototype);

// The baseline lacks these methods before construction. Do not enter its
// unsafe importer just to demonstrate rejection of a non-array allocation.
if (typeof load === "function" && typeof read === "function") {
  const cases: [string, any][] = [
    ["array", ["Address: IPv4 127.0.0.1", "Subnet: IPv6 2001:db8::/32"]],
    ["text", '["Range: IPv4 127.0.0.2-127.0.0.4"]'],
    ["empty-array", []],
    ["empty-text", "[]"],
    ["undefined", undefined],
    ["null", null],
    ["object", {}],
    ["array-like", { 0: "Address: IPv4 127.0.0.1", length: 1 }],
    ["number", 7],
    ["boolean", false],
    ["function", () => []],
    ["boxed-string", new String("[]")],
    ["member-number", [1]],
    ["member-object", [{}]],
    ["member-null", [null]],
    ["mixed-member", ["Address: IPv4 127.0.0.1", 1]],
    ["text-object", "{}"],
    ["text-null", "null"],
    ["text-number", "1"],
    ["text-member", '["Address: IPv4 127.0.0.1",1]'],
    ["syntax", "["],
    ["ignored", ["not a rule", "Subnet: IPv4 127.0.0.0/-1"]],
    ["v4-prefix", ["Address: IPv4 127.0.0.1", "Subnet: IPv4 127.0.0.0/33"]],
    ["v6-prefix", ["Address: IPv6 ::1", "Subnet: IPv6 ::/129"]],
    ["v4-range", ["Address: IPv4 127.0.0.1", "Range: IPv4 127.0.0.4-127.0.0.2"]],
    ["v6-range", ["Address: IPv6 ::1", "Range: IPv6 ::4-::2"]],
    ["invalid-ip", ["Address: IPv4 999.0.0.1"]],
    ["family-mismatch", ["Address: IPv4 ::1", "Address: IPv6 127.0.0.1"]],
    ["boundaries", ["Subnet: IPv4 127.0.0.1/32", "Subnet: IPv6 ::1/128"]],
    ["zero-prefix", ["Subnet: IPv4 0.0.0.0/0", "Subnet: IPv6 ::/0"]],
    ["array-proxy", new Proxy(["Address: IPv4 127.0.0.1"], {})],
  ];
  for (const [label, value] of cases) {
    const list = new BlockList();
    list.addAddress("192.0.2.1");
    try {
      console.log(label, "return", load.call(list, value));
    } catch (error: any) {
      console.log(label, "error", error.name, error.code);
    }
    console.log(label, "state", JSON.stringify(read.call(list)));
    console.log(label, "checks", list.check("192.0.2.1"), list.check("127.0.0.1"), list.check("::1", "ipv6"));
  }

  for (const value of [null, "[", [], [1]] as any[]) {
    try {
      load.call({}, value);
      console.log("receiver", JSON.stringify(value), "OK");
    } catch (error: any) {
      console.log("receiver", JSON.stringify(value), error.name, error.code);
    }
  }

  const list = new net.BlockList();
  const saved = list.fromJSON;
  console.log("saved", saved.call(list, ["Address: IPv4 127.0.0.1"]));
  console.log("direct", list.fromJSON('["Address: IPv6 ::1"]'));
  console.log("routes", JSON.stringify(list.toJSON()), list.check("127.0.0.1"), list.check("::1", "ipv6"));
  const snapshot = list.toJSON();
  snapshot.push("Address: IPv4 192.0.2.99");
  console.log("copy", list.check("192.0.2.99"), list.toJSON().length);
  const original = proto.toJSON;
  proto.toJSON = function () { return ["custom"]; };
  new net.BlockList();
  console.log("preserve", proto === net.BlockList.prototype, proto.toJSON === BlockList.prototype.toJSON, JSON.stringify(list.toJSON()));
  proto.toJSON = original;
}

// One address per history isolates category emission from native address-map
// traversal. Direct mutation and JSON import must expose the same ordering.
if (typeof load === "function" && typeof read === "function") {
  const histories = [
    ["address-subnet-range", ["address", "subnet", "range"]],
    ["address-range-subnet", ["address", "range", "subnet"]],
    ["subnet-address-range", ["subnet", "address", "range"]],
    ["subnet-range-address", ["subnet", "range", "address"]],
    ["range-address-subnet", ["range", "address", "subnet"]],
    ["range-subnet-address", ["range", "subnet", "address"]],
    ["metadata-order", ["range", "subnet", "range-next", "subnet-next", "range", "subnet"]],
  ] as [string, string[]][];
  for (const [label, history] of histories) {
    const direct = new BlockList();
    const input: string[] = [];
    for (const kind of history) {
      if (kind === "address") {
        direct.addAddress("192.0.2.1");
        input.push("Address: IPv4 192.0.2.1");
      } else if (kind === "subnet" || kind === "subnet-next") {
        const address = kind === "subnet" ? "198.51.100.0" : "203.0.113.0";
        direct.addSubnet(address, 24);
        input.push("Subnet: IPv4 " + address + "/24");
      } else {
        const start = kind === "range" ? "192.0.2.10" : "192.0.2.20";
        const end = kind === "range" ? "192.0.2.12" : "192.0.2.22";
        direct.addRange(start, end);
        input.push("Range: IPv4 " + start + "-" + end);
      }
    }
    const imported = new BlockList();
    console.log("category", label, "return", load.call(imported, input));
    console.log("category", label, "direct-rules", JSON.stringify(direct.rules));
    console.log("category", label, "direct-json", JSON.stringify(read.call(direct)));
    console.log("category", label, "import-rules", JSON.stringify(imported.rules));
    console.log("category", label, "import-json", JSON.stringify(read.call(imported)));
    console.log("category", label, "checks", direct.check("192.0.2.1"), imported.check("192.0.2.1"), direct.check("198.51.100.1"), imported.check("198.51.100.1"), direct.check("192.0.2.11"), imported.check("192.0.2.11"));
    const snapshot = imported.rules;
    snapshot.push("Address: IPv4 192.0.2.99");
    console.log("category", label, "copy", imported.check("192.0.2.99"), imported.rules.length, read.call(imported).length);
  }
}
