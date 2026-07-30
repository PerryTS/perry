import os from "node:os";

const nets = os.networkInterfaces();
const names = Object.keys(nets).sort();
const addresses = names.flatMap((name) => nets[name] || []);
console.log("names type:", Array.isArray(names), names.length > 0);
console.log("addresses present:", addresses.length > 0);
console.log(
  "address fields:",
  addresses.every((entry) =>
    typeof entry.address === "string" &&
    typeof entry.netmask === "string" &&
    (entry.family === "IPv4" || entry.family === "IPv6") &&
    /^([0-9a-f]{2}:){5}[0-9a-f]{2}$/i.test(entry.mac) &&
    typeof entry.internal === "boolean" &&
    entry.cidr.startsWith(`${entry.address}/`)
  ),
);
console.log(
  "scope shape:",
  addresses.every((entry) =>
    entry.family === "IPv6"
      ? typeof entry.scopeid === "number"
      : !("scopeid" in entry)
  ),
);
for (const name of names.slice(0, 2)) {
  const list = nets[name] || [];
  console.log("iface:", typeof name, Array.isArray(list), list.length > 0);
  const first: any = list[0];
  if (first) {
    console.log(
      "addr shape:",
      typeof first.address,
      typeof first.family,
      typeof first.internal,
    );
  }
}
