import { spawnSync } from "node:child_process";
import { mkdirSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const cwd = join(tmpdir(), `perry-child-process-optional-${process.pid}`);
mkdirSync(cwd);
const expected = realpathSync(cwd);

try {
  for (const [label, args] of [
    ["undefined", undefined],
    ["null", null],
    ["empty", []],
  ] as const) {
    const result = spawnSync(
      "node",
      args as any,
      {
        cwd,
        encoding: "utf8",
        env: { ...process.env, NODE_REPL_EXTERNAL_MODULE: "" },
        input: "process.stdout.write(process.cwd())\n",
      } as any,
    );
    console.log(`${label} status:`, result.status);
    console.log(`${label} cwd:`, result.stdout === expected);
    console.log(`${label} stderr empty:`, result.stderr === "");
  }
} finally {
  rmSync(cwd, { recursive: true, force: true });
}
