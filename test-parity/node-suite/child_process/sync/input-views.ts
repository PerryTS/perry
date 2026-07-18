import { spawnSync } from "node:child_process";

const code =
  "const fs=require('node:fs');const b=fs.readFileSync(0);process.stdout.write(b.toString('hex'))";

for (const [label, input] of [
  ["buffer", Buffer.from([0, 1, 254, 255])],
  ["uint8", new Uint8Array([2, 3, 252, 253])],
  ["data-view", new DataView(new Uint8Array([4, 5, 250, 251]).buffer)],
] as const) {
  try {
    const result = spawnSync("node", ["-e", code], { input, encoding: "utf8" });
    console.log(`${label} status:`, result.status);
    console.log(`${label} stdout:`, result.stdout);
    console.log(`${label} error:`, result.error?.code ?? "none");
  } catch (error: any) {
    console.log(`${label} throw:`, error?.constructor?.name, error?.code);
  }
}
