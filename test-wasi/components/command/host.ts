import { readFileSync } from "node:fs";
import { collect } from "perry/gc";
import { runComponentCommand } from "perry/component";

const bytes = readFileSync(process.argv[2]);
const directory = process.argv[3];
function check(ok: boolean, message: string): void {
  if (!ok) throw new Error(message);
}
function rejects(action: () => unknown, message: string): void {
  let rejected = false;
  try { action(); } catch (_) { rejected = true; }
  check(rejected, message);
}

check(runComponentCommand(bytes) === 0, "default grants leaked or normal return failed");
for (const status of [0, 1, 23, 255]) {
  collect();
  check(runComponentCommand(bytes, {
    wasi: { args: ["probe", "exit", String(status)] }
  }) === status, "explicit guest exit status");
}
check(runComponentCommand(bytes, {
  wasi: {
    args: ["probe", "granted", "alpha", "read-only"],
    env: { COMMAND_TOKEN: "alpha" },
    inheritStdout: true,
    inheritStderr: true,
    preopens: [{ hostPath: directory + "/alpha", guestPath: "/sandbox" }]
  }
}) === 0, "read-only command grant");
collect();
check(runComponentCommand(bytes, {
  wasi: {
    args: ["probe", "granted", "beta", "write"],
    env: { COMMAND_TOKEN: "beta" },
    preopens: [{ hostPath: directory + "/beta", guestPath: "/sandbox", readOnly: false }]
  }
}) === 0, "independent writable command grant");
check(readFileSync(directory + "/beta/output.txt", "utf8") === "beta", "command write missing");
rejects(() => readFileSync(directory + "/alpha/output.txt"), "read-only command wrote a file");
rejects(() => runComponentCommand(new Uint8Array([0, 1, 2])), "invalid component accepted");
rejects(() => runComponentCommand(bytes, { fuel: 0 }), "zero limit accepted");
rejects(() => runComponentCommand(bytes, { fuel: 10000, wasi: { args: ["probe", "spin"] } }), "fuel exhaustion ignored");
rejects(() => runComponentCommand(bytes, { wasi: { args: ["probe", "trap"] } }), "guest trap was an exit status");
collect();
check(runComponentCommand(bytes) === 0, "previous command contaminated a fresh store");
console.log("PASS WASI commands: returns, exit statuses, traps, isolated capabilities, fuel, host survival");
