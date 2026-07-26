import { BlockList } from "node:net";

const list = new BlockList();

console.log("class:", typeof BlockList, BlockList.length);
console.log(
  "identity:",
  BlockList.isBlockList(list),
  BlockList.isBlockList({}),
);
console.log(
  "methods:",
  ["addAddress", "addRange", "addSubnet", "check", "toJSON", "fromJSON"]
    .map((name) => `${name}:${typeof (list as any)[name]}`)
    .join(","),
);
