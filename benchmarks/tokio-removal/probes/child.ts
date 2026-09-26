import { exec, spawn } from "node:child_process";
exec("echo from-exec", (err, stdout) => {
  console.log("exec", err === null, stdout.trim());
  const p = spawn("sh", ["-c", "echo spawned; exit 3"]);
  let out = "";
  p.stdout.on("data", (d) => (out += d));
  p.on("close", (code) => console.log("spawn", out.trim(), code));
});
