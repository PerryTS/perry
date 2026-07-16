import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

function run(label: string, options: Record<string, any>): Promise<void> {
  return new Promise((resolve) => {
    const worker = new Worker("./execargv-worker.cjs", options);
    worker.on("message", (message) => console.log(label, message));
    worker.on(
      "error",
      (error: any) =>
        console.log(label, "error", error?.name, error?.code ?? ""),
    );
    worker.on("exit", (code) => {
      console.log(label, "exit", code);
      resolve();
    });
  });
}

async function main() {
  console.log("parent:", JSON.stringify(process.execArgv));
  await run("inherited:", {});
  await run("null:", { execArgv: null });
  await run("zero:", { execArgv: 0 });
  await run("false:", { execArgv: false });
  await run("empty:", { execArgv: [] });
  await run("custom:", { execArgv: ["--no-warnings"] });
  await run("nonstrings:", { execArgv: [1, null, true] as any });

  try {
    new Worker("./execargv-worker.cjs", {
      execArgv: ["--not-a-real-node-flag"],
    });
    console.log("invalid flag: ok");
  } catch (error: any) {
    console.log("invalid flag:", error?.name, error?.code ?? "");
  }
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
