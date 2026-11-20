import * as cp from "node:child_process";

const spawn = cp.spawn;
console.log("spawn type:", typeof spawn);

const shell = process.platform === "win32" ? "cmd" : "sh";
const child = spawn(shell);
console.log("child type:", typeof child);

child.on("exit", () => {
  console.log("exit");
});

child.stdin.end();
