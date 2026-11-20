import { type ChildProcess, spawn } from "node:child_process";
import { mkdirSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const cwd = join(tmpdir(), `perry-child-process-async-${process.pid}`);
mkdirSync(cwd);
const expectedCwd = realpathSync(cwd);

function close(child: ChildProcess): Promise<number | null> {
  return new Promise((resolve) => child.on("close", resolve));
}

try {
  const child = spawn(
    "node",
    [
      "-e",
      "process.stdout.write(JSON.stringify({ cwd: process.cwd(), value: process.env.PERRY_VALUE, empty: process.env.PERRY_EMPTY, missing: Object.hasOwn(process.env, 'PERRY_MISSING') }))",
    ],
    {
      cwd,
      env: {
        ...process.env,
        PERRY_VALUE: false as any,
        PERRY_EMPTY: "",
        PERRY_MISSING: undefined,
      },
    },
  );
  let stdout = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => {
    stdout += chunk;
  });
  const status = await close(child);
  console.log("status:", status);
  console.log("stdout present:", stdout.length > 0);
  if (stdout.length > 0) {
    const value = JSON.parse(stdout);
    console.log("cwd:", value.cwd === expectedCwd);
    console.log(
      "env:",
      value.value,
      JSON.stringify(value.empty),
      value.missing,
    );
  }
} finally {
  rmSync(cwd, { recursive: true, force: true });
}
