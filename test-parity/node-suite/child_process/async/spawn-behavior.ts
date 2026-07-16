import { type ChildProcess, spawn } from "node:child_process";

function close(child: ChildProcess): Promise<number | null> {
  return new Promise((resolve) => child.on("close", resolve));
}

{
  const child = spawn("node", [
    "-e",
    "process.stdout.write(process.argv[1])",
    "child-arg",
  ]);
  let stdout = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => (stdout += chunk));
  console.log("metadata pid type:", typeof child.pid);
  console.log("metadata spawnfile:", child.spawnfile);
  console.log("metadata spawnargs length:", child.spawnargs.length);
  console.log("metadata spawnargs command:", child.spawnargs[0]);
  console.log("metadata spawnargs flag:", child.spawnargs[1]);
  const code = await close(child);
  console.log("metadata close:", code, child.signalCode);
  console.log("metadata stdout:", stdout);
  console.log("metadata exitCode:", child.exitCode);
}

{
  const key = "PERRY_CHILD_DEFAULT_ENV";
  const previous = process.env[key];
  process.env[key] = "inherited-value";
  try {
    const child = spawn("node", [
      "-e",
      `process.stdout.write(process.env.${key} || 'missing')`,
    ]);
    let stdout = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => (stdout += chunk));
    console.log("default env status:", await close(child));
    console.log("default env value:", stdout);
  } finally {
    if (previous === undefined) delete process.env[key];
    else process.env[key] = previous;
  }
}

for (const [label, args] of [
  ["undefined", undefined],
  ["null", null],
  ["empty", []],
] as const) {
  const child = spawn(
    "node",
    args as any,
    { env: { ...process.env, PERRY_OPTIONAL_ARGS: label } } as any,
  );
  let stdout = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => (stdout += chunk));
  child.stdin.end("process.stdout.write(process.env.PERRY_OPTIONAL_ARGS)\n");
  console.log(`optional ${label} status:`, await close(child));
  console.log(`optional ${label} output:`, stdout);
}

{
  const options = {
    cwd: process.cwd(),
    env: { ...process.env, PERRY_IMMUTABLE: "yes" },
    stdio: ["ignore", "pipe", "pipe"] as any,
    windowsHide: true,
  };
  const before = {
    cwd: options.cwd,
    env: options.env.PERRY_IMMUTABLE,
    stdio: options.stdio.join(","),
    windowsHide: options.windowsHide,
  };
  const child = spawn(
    "node",
    ["-e", "process.stdout.write(process.env.PERRY_IMMUTABLE)"],
    options,
  );
  let stdout = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => (stdout += chunk));
  console.log("immutable status:", await close(child));
  console.log("immutable stdout:", stdout);
  console.log("immutable cwd:", options.cwd === before.cwd);
  console.log("immutable env:", options.env.PERRY_IMMUTABLE === before.env);
  console.log("immutable stdio:", options.stdio.join(",") === before.stdio);
  console.log(
    "immutable windowsHide:",
    options.windowsHide === before.windowsHide,
  );
}
