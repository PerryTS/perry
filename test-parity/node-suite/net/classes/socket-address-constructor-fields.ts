import { SocketAddress } from "node:net";

const cases: [string, any][] = [
  ["ipv4 default", {}],
  ["ipv6 default", { family: "ipv6" }],
  ["ipv4 explicit", { address: "192.0.2.1", family: "ipv4", port: 80 }],
  ["ipv6 explicit", { address: "::1", family: "ipv6", port: 443, flowlabel: 17 }],
  ["address type", { address: 1 }],
  ["family value", { family: "ip5" }],
  ["flowlabel negative", { family: "ipv6", flowlabel: -1 }],
  ["flowlabel fraction", { family: "ipv6", flowlabel: 1.5 }],
  ["family before address", { family: "ip5", address: 1 }],
];
for (const [label, options] of cases) {
  try {
    const value = new SocketAddress(options);
    console.log(label, value.address, value.family, value.port, value.flowlabel);
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}

const reads: string[] = [];
const value = new SocketAddress({
  get family() { reads.push("family"); return "ipv6"; },
  get address() { reads.push("address"); return "::1"; },
  get port() { reads.push("port"); return 1234; },
  get flowlabel() { reads.push("flowlabel"); return 7; },
});
console.log("getter order", reads.join(","));
console.log("getter values", value.address, value.family, value.port, value.flowlabel);
