// family_reach: a Buffer arrives from a native (a file read) without the
// program allocating one. Under PERRY_FACTS_SABOTAGE=famreach:typedarray the
// witness at the allocation of every byte cell must report it ("family").
import * as fs from "fs";
const bytes = fs.readFileSync("/proc/self/comm");
console.log(bytes.length > 0);
