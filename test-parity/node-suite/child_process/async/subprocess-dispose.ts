import { spawn } from "node:child_process";

const command = process.platform === "win32" ? "cmd" : "sleep";
const args =
  process.platform === "win32"
    ? ["/c", "ping -n 60 127.0.0.1 >NUL"]
    : ["60"];
const child = spawn(command, args);
const dispose = (child as any)[Symbol.dispose];

child.on("exit", (code, signal) => {
  console.log("dispose exit:", code, signal);
});

console.log("dispose type:", typeof dispose);
console.log("dispose length:", dispose?.length);
console.log("dispose name:", dispose?.name);
console.log("dispose return undefined:", dispose.call(child) === undefined);
