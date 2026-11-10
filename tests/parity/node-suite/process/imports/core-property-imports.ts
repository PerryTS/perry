// #3946: node:process core properties resolve through named + namespace
// imports, matching the global `process` object (not `undefined`).
import { pid, ppid, arch, platform, version, versions, argv, env } from "node:process";
import * as ns from "node:process";

console.log("named", typeof pid, typeof ppid, typeof arch, typeof platform, typeof version, typeof versions, Array.isArray(argv), typeof env);
console.log("namespace", typeof ns.pid, typeof ns.arch, typeof ns.platform, typeof ns.version);
console.log("agree", arch === ns.arch, platform === ns.platform, version === ns.version);
console.log("global-agree", pid === process.pid, arch === process.arch, platform === process.platform);
