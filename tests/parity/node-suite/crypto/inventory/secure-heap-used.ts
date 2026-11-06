import * as crypto from "node:crypto";

const used = crypto.secureHeapUsed();
console.log("secureHeap type:", typeof used);
console.log("secureHeap keys:", Object.keys(used).sort().join(","));
console.log("secureHeap total:", used.total);
console.log("secureHeap used:", used.used);
console.log("secureHeap utilization:", used.utilization);
console.log("secureHeap min:", used.min);
console.log("secureHeap function name:", crypto.secureHeapUsed.name);
