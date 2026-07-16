import { execFileSync, spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

const missing = join(tmpdir(), `perry-missing-cwd-${process.pid}`);
const missingCwd = spawnSync("node", ["-e", ""], { cwd: missing });
console.log("missing cwd status:", missingCwd.status);
console.log("missing cwd signal:", missingCwd.signal);
console.log("missing cwd pid type:", typeof missingCwd.pid);
console.log(
  "missing cwd error:",
  missingCwd.error?.constructor?.name,
  missingCwd.error?.code,
);
console.log("missing cwd error path:", missingCwd.error?.path);

try {
  execFileSync("node", ["-e", ""], { cwd: missing });
  console.log("execFileSync missing cwd: no throw");
} catch (error: any) {
  console.log(
    "execFileSync missing cwd:",
    error?.constructor?.name,
    error?.code,
  );
  console.log("execFileSync missing status:", error?.status);
  console.log("execFileSync missing signal:", error?.signal);
}

function reportShell(label: string, command: string) {
  const result = spawnSync(command, { shell: true, encoding: "utf8" });
  console.log(`${label} status:`, result.status);
  console.log(`${label} signal:`, result.signal);
  console.log(`${label} error:`, result.error?.code ?? "none");
  console.log(`${label} stdout:`, JSON.stringify(result.stdout));
  console.log(`${label} stderr empty:`, result.stderr?.length === 0);
}

reportShell("shell success", `node -e "process.stdout.write('shell-ok')"`);
reportShell(
  "shell failure",
  `node -e "process.stderr.write('shell-err');process.exit(7)"`,
);

function reportUnicodeMaxBuffer(label: string, maxBuffer: number) {
  try {
    const stdout = execFileSync(
      "node",
      ["-e", "process.stdout.write('中文测试')"],
      { encoding: "utf8", maxBuffer },
    );
    console.log(`${label} result:`, JSON.stringify(stdout));
  } catch (error: any) {
    console.log(`${label} error:`, error.name, error.code);
    console.log(`${label} status:`, error.status);
    console.log(`${label} signal:`, error.signal);
    console.log(`${label} stdout:`, JSON.stringify(String(error.stdout)));
    console.log(`${label} stderr:`, JSON.stringify(String(error.stderr)));
  }
}

reportUnicodeMaxBuffer("unicode exact bytes", 12);
reportUnicodeMaxBuffer("unicode one byte short", 11);
reportUnicodeMaxBuffer("unicode one character", 3);
reportUnicodeMaxBuffer("unicode zero", 0);
