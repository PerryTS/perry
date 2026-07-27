import * as readline from "node:readline";

console.log(Object.keys(readline).sort().join(","));
console.log(typeof readline.Interface, readline.Interface?.length);
console.log(readline.ReadLine);
