import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

async function main() {
  const worker = new Worker("./captured-output-worker.cjs", {
    stdout: true,
    stderr: true,
  });
  let stdout = "";
  let stderr = "";
  let message = "missing";

  worker.stdout.setEncoding("utf8");
  worker.stderr.setEncoding("utf8");
  worker.stdout.on("data", (chunk) => stdout += chunk);
  worker.stderr.on("data", (chunk) => stderr += chunk);
  worker.on("message", (value) => message = value);

  const exit = new Promise<number>((resolve) => worker.on("exit", resolve));
  const stdoutEnd = new Promise<void>((resolve) =>
    worker.stdout.on("end", resolve)
  );
  const stderrEnd = new Promise<void>((resolve) =>
    worker.stderr.on("end", resolve)
  );
  const [code] = await Promise.all([exit, stdoutEnd, stderrEnd]);

  console.log("message:", message);
  console.log("stdout:", stdout.trim());
  console.log("stderr:", stderr.trim());
  console.log("exit:", code);
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
