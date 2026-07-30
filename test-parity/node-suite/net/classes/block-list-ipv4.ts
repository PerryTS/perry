import { BlockList } from "node:net";

const list = new BlockList();
list.addAddress("10.0.0.1");
list.addRange("10.0.1.1", "10.0.1.3");
list.addSubnet("10.0.2.0", 24);

for (
  const address of [
    "10.0.0.1",
    "10.0.0.2",
    "10.0.1.2",
    "10.0.1.4",
    "10.0.2.255",
    "bad",
  ]
) {
  console.log(address, list.check(address));
}

console.log(JSON.stringify(list.rules));
