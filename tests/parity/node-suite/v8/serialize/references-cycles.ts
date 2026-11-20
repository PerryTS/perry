import { deserialize, serialize } from "node:v8";

const shared = { marker: "shared" };
const root: any = { left: shared, right: shared, array: [shared] };
root.self = root;
root.array.push(root.array);

const result: any = deserialize(serialize(root));
console.log("root cycle:", result.self === result);
console.log(
  "shared object:",
  result.left === result.right,
  result.left === result.array[0],
);
console.log("array cycle:", result.array[1] === result.array);
console.log("fresh graph:", result !== root, result.left !== shared);
