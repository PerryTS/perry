import { BlockList } from "node:net";

const list = new BlockList();
list.addAddress("::1", "ipv6");
list.addRange("2001:db8::1", "2001:db8::3", "ipv6");
list.addSubnet("2001:db8:1::", 48, "ipv6");

for (
  const address of [
    "::1",
    "::2",
    "2001:db8::2",
    "2001:db8::4",
    "2001:db8:1::ffff",
  ]
) {
  console.log(address, list.check(address, "ipv6"));
}

console.log(JSON.stringify(list.rules));
