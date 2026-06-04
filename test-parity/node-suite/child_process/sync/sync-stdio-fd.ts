import { execFileSync, execSync, spawnSync } from "node:child_process";
import { closeSync, mkdirSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function text(value: unknown) {
  if (value === null) return "null";
  if (value === undefined) return "undefined";
  if (Buffer.isBuffer(value)) return `Buffer:${value.toString()}`;
  return String(value);
}

function outputText(output: unknown) {
  return Array.isArray(output) ? output.map(text).join("|") : text(output);
}

function readText(path: string) {
  return readFileSync(path, "utf8");
}

const dir = join(tmpdir(), `perry-sync-stdio-fd-${process.pid}`);
rmSync(dir, { recursive: true, force: true });
mkdirSync(dir);

try {
  const outPath = join(dir, "out.txt");
  const errPath = join(dir, "err.txt");
  const stdinPath = join(dir, "stdin.txt");
  rmSync(outPath, { force: true });
  rmSync(errPath, { force: true });

  let outFd = openSync(outPath, "w");
  let errFd = openSync(errPath, "w");
  const spawnResult = spawnSync("sh", ["-c", "printf out; printf err >&2; exit 7"], {
    stdio: ["ignore", outFd, errFd],
    encoding: "utf8",
  });
  closeSync(outFd);
  closeSync(errFd);

  console.log("spawn status:", spawnResult.status);
  console.log("spawn stdout:", text(spawnResult.stdout));
  console.log("spawn stderr:", text(spawnResult.stderr));
  console.log("spawn output:", outputText(spawnResult.output));
  console.log("spawn files:", readText(outPath), readText(errPath));

  outFd = openSync(outPath, "w");
  errFd = openSync(errPath, "w");
  const execFileResult = execFileSync("sh", ["-c", "printf fileout; printf fileerr >&2"], {
    stdio: ["ignore", outFd, errFd],
    encoding: "utf8",
  });
  closeSync(outFd);
  closeSync(errFd);

  console.log("execFile return:", text(execFileResult));
  console.log("execFile files:", readText(outPath), readText(errPath));

  outFd = openSync(outPath, "w");
  errFd = openSync(errPath, "w");
  const execResult = execSync("printf execout; printf execerr >&2", {
    stdio: ["ignore", outFd, errFd],
    encoding: "utf8",
  });
  closeSync(outFd);
  closeSync(errFd);

  console.log("exec return:", text(execResult));
  console.log("exec files:", readText(outPath), readText(errPath));

  outFd = openSync(outPath, "w");
  errFd = openSync(errPath, "w");
  try {
    execFileSync("sh", ["-c", "printf failout; printf failerr >&2; exit 9"], {
      stdio: ["ignore", outFd, errFd],
      encoding: "utf8",
    });
  } catch (err: any) {
    console.log("execFile throw status:", err.status);
    console.log("execFile throw stdout:", text(err.stdout));
    console.log("execFile throw stderr:", text(err.stderr));
    console.log("execFile throw output:", outputText(err.output));
  }
  closeSync(outFd);
  closeSync(errFd);

  console.log("execFile throw files:", readText(outPath), readText(errPath));

  writeFileSync(stdinPath, "fdinput");
  const inFd = openSync(stdinPath, "r");
  const stdinResult = spawnSync("cat", [], {
    stdio: [inFd, "pipe", "pipe"],
    encoding: "utf8",
  });
  closeSync(inFd);

  console.log("spawn stdin fd stdout:", text(stdinResult.stdout));
  console.log("spawn stdin fd output:", outputText(stdinResult.output));
} finally {
  rmSync(dir, { recursive: true, force: true });
}
